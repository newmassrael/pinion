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
use std::task::{Context, Waker};

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

/// R761.1 §5.22 — production [`LocalSpawner`]: the UI-thread cooperative
/// task pump the R26/R37 spec anticipated ("the framework integrator
/// spawns the future"). [`spawn_local`](LocalSpawner::spawn_local)
/// **enqueues** a future; [`poll`](Self::poll), driven once per frame by
/// the shell, advances every queued future one step and drops the ones
/// that completed.
///
/// This is what makes a *deferred* future (one that yields `Pending`
/// until off-thread work completes — e.g. a native `rfd::AsyncFileDialog`
/// awaiting an `xdg-desktop-portal` D-Bus reply) actually resolve on the
/// UI thread: each frame re-polls it until it is `Ready`, then the
/// wrapping [`Resource::fetch_with`] driver writes the value through its
/// [`FetchToken`]. A binding's immediately-ready future (the scripted
/// test path) completes on the first poll.
///
/// ## Waker
///
/// v1 polls with the stable `Waker::noop`, so a queued future is only
/// advanced when [`poll`](Self::poll) runs — never self-woken. The shell
/// therefore keeps requesting frames while [`has_pending`](Self::has_pending)
/// is `true` (same "stay awake while active" contract as
/// [`Owner::any_animation_active`](super::owner::Owner)). A wake-channel
/// waker that requests a single redraw on completion (avoiding the
/// busy-poll while a dialog is open) is a forward refinement; modal
/// dialogs are short-lived, so the busy-poll cost is bounded.
///
/// ## Re-entrancy
///
/// [`poll`](Self::poll) takes the queue out before polling, so a future
/// that spawns another task during its own poll (e.g. a completion that
/// triggers an `Effect` that starts a follow-up fetch) does not
/// re-entrantly borrow the queue. Survivors and newly-spawned tasks are
/// merged back afterward.
///
/// Single-thread (`RefCell`, `!Send`): matches the `!Send` payload
/// contract of [`LocalSpawner`] and the UI-thread reactive runtime (§6.3).
#[derive(Default)]
pub struct LocalTaskPump {
    pending: std::cell::RefCell<Vec<Pin<Box<dyn Future<Output = ()> + 'static>>>>,
}

impl LocalTaskPump {
    /// Construct an empty pump.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of queued (not-yet-completed) tasks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.borrow().len()
    }

    /// `true` when no tasks are queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }

    /// `true` when at least one task is still queued — the shell reads
    /// this to keep the frame loop awake while async work is in flight.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.is_empty()
    }

    /// Advance every queued future one poll. Futures that resolve are
    /// dropped; futures that yield `Pending` are retained for the next
    /// frame. Returns [`has_pending`](Self::has_pending) *after* this
    /// poll so the caller can decide whether to schedule another frame.
    pub fn poll(&self) -> bool {
        // Take the queue out so a task that spawns another during its
        // own poll does not re-entrantly borrow `pending`.
        let mut taken = core::mem::take(&mut *self.pending.borrow_mut());
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        taken.retain_mut(|task| task.as_mut().poll(&mut cx).is_pending());

        // Merge survivors with anything spawned during the poll (which
        // landed in the now-empty `pending`).
        let mut pending = self.pending.borrow_mut();
        taken.append(&mut pending);
        *pending = taken;
        !pending.is_empty()
    }
}

impl core::fmt::Debug for LocalTaskPump {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalTaskPump")
            .field("pending", &self.len())
            .finish()
    }
}

impl LocalSpawner for LocalTaskPump {
    /// Enqueue `future`; it is driven to completion by subsequent
    /// [`poll`](LocalTaskPump::poll) calls (unlike a `block_on` spawner,
    /// this returns immediately without advancing the future).
    fn spawn_local(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) {
        self.pending.borrow_mut().push(future);
    }
}

/// R761.1 §5.22 — hook returning the active owner scope's shared
/// [`LocalTaskPump`]. Bindings drive a [`Resource`] with a deferred
/// future via `resource.fetch_with(&*use_local_task_pump(), fut)`; the
/// shell polls the *same* pump each frame (through
/// [`Owner::local_task_pump`](super::owner::Owner::local_task_pump)), so
/// the future actually advances. Both the immediately-ready scripted
/// path and a deferred native (`rfd`) future resolve through this one
/// spawner — no per-binding `block_on` helper.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same shape as
/// the other `use_*` hooks).
#[must_use]
pub fn use_local_task_pump() -> Rc<LocalTaskPump> {
    super::owner::Owner::current()
        .expect("use_local_task_pump requires an active Owner scope")
        .local_task_pump()
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

    // ────────────────────────────────────────────────────────────────
    // R761.1 §5.22 — LocalTaskPump (production deferred-future driver)
    // ────────────────────────────────────────────────────────────────

    /// Future that yields `Pending` `remaining` times, then `Ready(value)`.
    /// Models a deferred task (e.g. a native dialog awaiting a portal
    /// reply) without a real async runtime.
    struct Defer {
        remaining: Cell<u32>,
        value: i32,
    }

    impl Future for Defer {
        type Output = i32;
        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<i32> {
            if self.remaining.get() == 0 {
                Poll::Ready(self.value)
            } else {
                self.remaining.set(self.remaining.get() - 1);
                Poll::Pending
            }
        }
    }

    #[test]
    fn r761_1_pump_drives_ready_future_in_one_poll() {
        let pump = LocalTaskPump::new();
        let flag = Rc::new(Cell::new(false));
        let f = Rc::clone(&flag);
        pump.spawn_local(Box::pin(async move { f.set(true); }));
        assert!(pump.has_pending());
        assert_eq!(pump.len(), 1);
        assert!(!pump.poll(), "an immediately-ready future completes in one poll");
        assert!(pump.is_empty());
        assert!(flag.get(), "the task body ran");
    }

    #[test]
    fn r761_1_pump_drives_deferred_future_across_polls() {
        let pump = LocalTaskPump::new();
        let out = Rc::new(Cell::new(0));
        let o = Rc::clone(&out);
        pump.spawn_local(Box::pin(async move {
            let v = Defer {
                remaining: Cell::new(2),
                value: 42,
            }
            .await;
            o.set(v);
        }));
        assert!(pump.poll(), "still pending after poll 1");
        assert_eq!(out.get(), 0);
        assert!(pump.poll(), "still pending after poll 2");
        assert_eq!(out.get(), 0);
        assert!(!pump.poll(), "deferred future resolves on poll 3");
        assert_eq!(out.get(), 42, "the awaited value lands");
        assert!(pump.is_empty());
    }

    #[test]
    fn r761_1_resource_loads_through_pump_loading_then_ready() {
        // The R761 file-dialog shape: a deferred future feeds a Resource
        // through fetch_with + the pump. State stays Loading across the
        // Pending polls, then flips to Ready — exactly the lifecycle a
        // native RfdFileDialog future drives (the scripted mock collapses
        // to the one-poll path above).
        let pump = LocalTaskPump::new();
        let r = Resource::<i32, String>::ready(0);
        r.fetch_with(&pump, async {
            let v = Defer {
                remaining: Cell::new(1),
                value: 7,
            }
            .await;
            Ok::<i32, String>(v)
        });
        assert_eq!(r.state(), ResourceState::Loading, "fetch_with marks Loading");
        assert_eq!(pump.len(), 1);
        assert!(pump.poll(), "Defer still pending after poll 1");
        assert_eq!(r.state(), ResourceState::Loading);
        assert!(!pump.poll(), "resolves on poll 2");
        assert_eq!(r.state(), ResourceState::Ready(7));
        assert!(pump.is_empty());
    }

    #[test]
    fn r761_1_pump_is_reentrancy_safe_when_task_spawns_during_poll() {
        let pump = Rc::new(LocalTaskPump::new());
        let inner = Rc::clone(&pump);
        let ran = Rc::new(Cell::new(0));
        let r1 = Rc::clone(&ran);
        let r2 = Rc::clone(&ran);
        pump.spawn_local(Box::pin(async move {
            r1.set(r1.get() + 1);
            // Spawn a follow-up while this task is being polled — must
            // not re-entrantly borrow the queue.
            inner.spawn_local(Box::pin(async move { r2.set(r2.get() + 1); }));
        }));
        assert!(pump.poll(), "follow-up task queued during poll stays pending");
        assert_eq!(ran.get(), 1);
        assert!(!pump.poll(), "follow-up task runs on the next poll");
        assert_eq!(ran.get(), 2);
    }

    #[test]
    fn r761_1_pump_default_and_debug() {
        let pump = LocalTaskPump::default();
        assert!(pump.is_empty());
        assert!(format!("{pump:?}").contains("pending: 0"));
    }
}
