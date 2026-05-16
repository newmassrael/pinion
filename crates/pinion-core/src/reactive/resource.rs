//! `Resource<T, E>` — async-state reactive primitive (§5.22 R26).
//!
//! R26 caveats locked here:
//! - `enum {Loading, Ready(T), Error(E)}` state machine, surfaced as a
//!   `Signal<ResourceState<T, E>>` so reads auto-subscribe the current scope
//! - cancel old task on refetch: a monotonic generation counter invalidates
//!   in-flight `FetchToken`s; only the current-generation token can mutate
//!   state, so stale completions become no-ops
//! - auto-refetch on dep change: deferred — wired by `Effect` (§5.23, R28).
//!   v0 exposes the manual `invalidate` → `FetchToken` pattern so the same
//!   cancellation primitive composes with whatever spawner the framework or
//!   user code provides.
//!
//! Async-runtime-agnostic by design: no tokio dependency in `pinion-core`.
//! The framework integrator (or the future Effect runtime) spawns the future
//! that wraps `token.complete()` / `token.fail()`. §6.3 keeps tokio at the
//! `pinion-rpc` boundary.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

use super::signal::Signal;

/// Task-spawning hook for [`Resource::fetch_with`]. Implementors plug in
/// their async runtime — typically `tokio::task::spawn_local`,
/// `wasm_bindgen_futures::spawn_local`, or a single-thread test executor.
///
/// Single-thread by design (`!Send` future): the §5.22 v0 reactive runtime
/// is thread-local so the spawned future runs on the same thread as the
/// signal it will mutate. Cross-thread carry-forward to §5.29 (R34
/// `SyncSignal`) will introduce a parallel `RemoteSpawner` trait.
pub trait LocalSpawner {
    /// Take ownership of `future` and arrange for it to be driven to
    /// completion on the current thread. The future is `'static` because it
    /// usually outlives this call.
    fn spawn_local(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>);
}

/// State of an async-loaded resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceState<T, E> {
    /// Fetch in progress; no value available yet.
    Loading,
    /// Fetch succeeded with `T`.
    Ready(T),
    /// Fetch failed with `E`.
    Error(E),
}

/// Reactive wrapper around an async-loaded value with cancellation semantics.
pub struct Resource<T, E>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    state: Signal<ResourceState<T, E>>,
    generation: Rc<Cell<u64>>,
}

impl<T, E> Resource<T, E>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// Construct a resource that starts in `Loading`.
    #[must_use]
    pub fn loading() -> Self {
        Self {
            state: Signal::new(ResourceState::Loading),
            generation: Rc::new(Cell::new(0)),
        }
    }

    /// Construct a resource that starts in `Ready(value)`. Useful for tests
    /// and for synchronous initial data.
    #[must_use]
    pub fn ready(value: T) -> Self {
        Self {
            state: Signal::new(ResourceState::Ready(value)),
            generation: Rc::new(Cell::new(0)),
        }
    }

    /// Construct a resource that starts in `Error(err)`.
    #[must_use]
    pub fn failed(err: E) -> Self {
        Self {
            state: Signal::new(ResourceState::Error(err)),
            generation: Rc::new(Cell::new(0)),
        }
    }

    /// Read the current state. Auto-subscribes the active reactive scope.
    #[must_use]
    pub fn state(&self) -> ResourceState<T, E> {
        self.state.get()
    }

    /// Begin a new fetch generation. The state transitions to `Loading` (if
    /// not already there) and a `FetchToken` is returned; only the holder of
    /// the current-generation token can mutate the state. Any prior in-flight
    /// token becomes a no-op — this is how the spec's "cancel old task on
    /// refetch" is realized in single-thread v0.
    pub fn invalidate(&self) -> FetchToken<T, E> {
        let new_gen = self.generation.get().wrapping_add(1);
        self.generation.set(new_gen);
        self.state.set(ResourceState::Loading);
        FetchToken {
            generation: new_gen,
            state: self.state.clone(),
            current_generation: Rc::clone(&self.generation),
        }
    }

    /// Spawn `future` on `spawner`, marking this resource as `Loading` for
    /// the duration. On completion the future writes `Ready(v)` or
    /// `Error(e)` through a `FetchToken`; if a newer `invalidate` /
    /// `fetch_with` runs in the meantime, the older future's eventual
    /// completion becomes a no-op (cancellation per R26 caveat).
    ///
    /// The future is `'static` because it almost always outlives this call
    /// — the spawner schedules it onto the runtime's task queue. The
    /// runtime is expected to drive the future on the same thread that
    /// owns this `Resource` (`!Send` payload).
    pub fn fetch_with<F, S>(&self, spawner: &S, future: F)
    where
        F: Future<Output = Result<T, E>> + 'static,
        S: LocalSpawner,
    {
        let token = self.invalidate();
        let driver: Pin<Box<dyn Future<Output = ()> + 'static>> = Box::pin(async move {
            match future.await {
                Ok(value) => token.complete(value),
                Err(err) => token.fail(err),
            }
        });
        spawner.spawn_local(driver);
    }

    /// Current generation. Each `invalidate` advances it by one.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }
}

impl<T, E> Clone for Resource<T, E>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            generation: Rc::clone(&self.generation),
        }
    }
}

/// Single-use completion handle bound to the generation in which it was
/// minted. Once a newer `invalidate` runs, the token's writes become no-ops.
#[must_use = "FetchToken does nothing unless completed via complete() or fail()"]
pub struct FetchToken<T, E>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    generation: u64,
    state: Signal<ResourceState<T, E>>,
    current_generation: Rc<Cell<u64>>,
}

impl<T, E> FetchToken<T, E>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// Successfully complete the fetch. No-op if a newer `invalidate` has run.
    pub fn complete(self, value: T) {
        if self.current_generation.get() == self.generation {
            self.state.set(ResourceState::Ready(value));
        }
    }

    /// Fail the fetch with `err`. No-op if a newer `invalidate` has run.
    pub fn fail(self, err: E) {
        if self.current_generation.get() == self.generation {
            self.state.set(ResourceState::Error(err));
        }
    }

    /// Whether this token still owns the current generation. Useful for an
    /// async fetcher to short-circuit work before committing a result.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.current_generation.get() == self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::owner::Owner;
    use std::task::{Context, Poll, Waker};

    /// Minimal `LocalSpawner` for tests: polls the future to completion on
    /// the current thread using `Waker::noop` (stabilized in Rust 1.85).
    /// Only immediately-ready futures are supported — anything that yields
    /// `Pending` would imply a real executor is needed, which tests do not
    /// provide. We panic on `Pending` so a misuse surfaces loudly.
    struct BlockingSpawner;

    impl LocalSpawner for BlockingSpawner {
        fn spawn_local(&self, mut future: Pin<Box<dyn Future<Output = ()> + 'static>>) {
            let waker: &Waker = Waker::noop();
            let mut cx = Context::from_waker(waker);
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {}
                Poll::Pending => {
                    panic!(
                        "BlockingSpawner only supports immediately-ready futures; \
                         received Pending — use a real executor for awaiting work"
                    );
                }
            }
        }
    }

    #[test]
    fn loading_resource_starts_loading() {
        let r = Resource::<i32, String>::loading();
        assert_eq!(r.state(), ResourceState::Loading);
        assert_eq!(r.generation(), 0);
    }

    #[test]
    fn ready_constructor_starts_ready() {
        let r = Resource::<i32, String>::ready(42);
        assert_eq!(r.state(), ResourceState::Ready(42));
    }

    #[test]
    fn failed_constructor_starts_error() {
        let r = Resource::<i32, String>::failed(String::from("boom"));
        assert_eq!(r.state(), ResourceState::Error(String::from("boom")));
    }

    #[test]
    fn invalidate_transitions_to_loading_and_advances_generation() {
        let r = Resource::<i32, String>::ready(1);
        assert_eq!(r.generation(), 0);
        let _token = r.invalidate();
        assert_eq!(r.state(), ResourceState::Loading);
        assert_eq!(r.generation(), 1);
    }

    #[test]
    fn token_complete_sets_ready() {
        let r = Resource::<i32, String>::loading();
        let token = r.invalidate();
        assert!(token.is_current());
        token.complete(7);
        assert_eq!(r.state(), ResourceState::Ready(7));
    }

    #[test]
    fn token_fail_sets_error() {
        let r = Resource::<i32, String>::loading();
        let token = r.invalidate();
        token.fail(String::from("network"));
        assert_eq!(r.state(), ResourceState::Error(String::from("network")));
    }

    #[test]
    fn stale_token_complete_is_noop() {
        let r = Resource::<i32, String>::loading();
        let stale = r.invalidate();
        let fresh = r.invalidate();
        assert!(!stale.is_current());
        assert!(fresh.is_current());
        stale.complete(99);
        // State should still reflect the second `invalidate` (Loading), not 99.
        assert_eq!(r.state(), ResourceState::Loading);
        fresh.complete(7);
        assert_eq!(r.state(), ResourceState::Ready(7));
    }

    #[test]
    fn stale_token_fail_is_noop() {
        let r = Resource::<i32, String>::loading();
        let stale = r.invalidate();
        let _fresh = r.invalidate();
        stale.fail(String::from("ignored"));
        assert_eq!(r.state(), ResourceState::Loading);
    }

    #[test]
    fn reading_state_inside_owner_scope_subscribes() {
        let r = Resource::<i32, String>::loading();
        let owner = Owner::new();
        owner.run(|| {
            assert_eq!(r.state(), ResourceState::Loading);
        });
        assert!(!owner.is_dirty());
        let token = r.invalidate();
        token.complete(42);
        assert!(owner.is_dirty());
    }

    #[test]
    fn invalidate_chain_dirties_observer_each_completion() {
        let r = Resource::<i32, String>::loading();
        let owner = Owner::new();
        owner.run(|| {
            let _ = r.state();
        });
        let t1 = r.invalidate();
        t1.complete(1);
        assert!(owner.is_dirty());
        owner.clear_dirty();
        let t2 = r.invalidate();
        t2.complete(2);
        assert!(owner.is_dirty());
        assert_eq!(r.state(), ResourceState::Ready(2));
    }

    #[test]
    fn clone_shares_underlying_state() {
        let r = Resource::<i32, String>::ready(10);
        let alias = r.clone();
        let token = alias.invalidate();
        token.complete(20);
        assert_eq!(r.state(), ResourceState::Ready(20));
        assert_eq!(alias.state(), ResourceState::Ready(20));
    }

    // ---- R37.6 #12: fetch_with + LocalSpawner ----------------------------

    #[test]
    fn fetch_with_ready_future_transitions_to_ready() {
        let r = Resource::<i32, String>::loading();
        r.fetch_with(&BlockingSpawner, async { Ok::<i32, String>(99) });
        assert_eq!(r.state(), ResourceState::Ready(99));
        assert_eq!(r.generation(), 1);
    }

    #[test]
    fn fetch_with_err_future_transitions_to_error() {
        let r = Resource::<i32, String>::loading();
        r.fetch_with(
            &BlockingSpawner,
            async { Err::<i32, _>(String::from("network down")) },
        );
        assert_eq!(r.state(), ResourceState::Error(String::from("network down")));
    }

    /// Test-only spawner that queues futures without polling them, so the
    /// test can simulate "spawn, then drive later" runtime ordering and
    /// observe `FetchToken` generation cancellation in action.
    struct DeferredSpawner {
        queue: std::cell::RefCell<Vec<Pin<Box<dyn Future<Output = ()> + 'static>>>>,
    }

    impl LocalSpawner for DeferredSpawner {
        fn spawn_local(&self, fut: Pin<Box<dyn Future<Output = ()> + 'static>>) {
            self.queue.borrow_mut().push(fut);
        }
    }

    #[test]
    fn fetch_with_overlapping_calls_only_keeps_latest() {
        // Two fetch_with calls in a row: the first's effect must be voided
        // because the second's `invalidate` advanced the generation before
        // the spawner ran the first future.
        let r = Resource::<i32, String>::loading();
        let spawner = DeferredSpawner {
            queue: std::cell::RefCell::new(Vec::new()),
        };
        r.fetch_with(&spawner, async { Ok::<i32, String>(1) });
        r.fetch_with(&spawner, async { Ok::<i32, String>(2) });
        assert_eq!(spawner.queue.borrow().len(), 2);
        // Drive both queued futures synchronously. First completion's token
        // is stale (gen 1 vs current 2), so its writes no-op; second
        // completion (gen 2) lands.
        let waker: &Waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        for mut fut in spawner.queue.into_inner() {
            assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(())));
        }
        assert_eq!(r.state(), ResourceState::Ready(2));
    }

    #[test]
    fn fetch_with_observer_sees_ready_after_completion() {
        let r = Resource::<i32, String>::loading();
        let owner = Owner::new();
        owner.run(|| {
            let _ = r.state();
        });
        assert!(!owner.is_dirty());
        r.fetch_with(&BlockingSpawner, async { Ok::<i32, String>(7) });
        assert!(owner.is_dirty());
        assert_eq!(r.state(), ResourceState::Ready(7));
    }
}
