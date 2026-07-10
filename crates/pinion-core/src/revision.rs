//! Monotonic OCC version token for the live scene (§5.34 R40.4).
//!
//! The optimistic-concurrency-control token attached to a running
//! pinion application's scene. Captured at `scene/propose_change`
//! time and stored in the preview ledger; re-checked at
//! `scene/apply_preview` time. A mismatch indicates the scene has
//! mutated underneath an in-flight preview, and the preview's effect
//! may no longer correspond to what the AI agent reasoned about at
//! propose time.
//!
//! # Bump policy
//!
//! Owners of a `SceneRevision` are responsible for calling
//! [`bump`](SceneRevision::bump) after any mutation that could
//! affect what `scene/query` / `scene/snapshot` / `scene/locate`
//! return. `pinion-rpc::dispatch` bumps automatically after a
//! mutating handler succeeds (`click`, `rewind`, `invoke`); the
//! embedding app additionally bumps after any direct mutation that
//! bypasses the dispatcher (e.g. winit-side input forwarded straight
//! to a widget's `External::invoke`).
//!
//! Bumping conservatively (a few unnecessary bumps) is preferred to
//! bumping not enough: a stale revision token causes false-positive
//! conflicts which the AI agent can recover from by re-proposing; a
//! missed bump causes false-negative apply success, applying stale
//! proposals against a scene that has silently moved on.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// A wake observer fired after every [`SceneRevision::bump`] with the new
/// value — the seam a change-driven consumer (an async `scene/waitFor`
/// registry) hooks so that **every** revision advance, from any bump site,
/// reaches it (R1270 §6.3). `Send + Sync` because a bump can happen on any
/// thread (dispatch's UI thread, an external-data producer thread).
pub type RevisionObserver = Box<dyn Fn(u64) + Send + Sync>;

/// Monotonic version counter attached to the live scene (§5.34 R40.4).
///
/// Thread-safe — backed by a single [`AtomicU64`]. Cloning is *not*
/// implemented because the revision is a per-scene singleton; the
/// embedder owns one and shares `&SceneRevision` references to it (or an
/// `Arc<SceneRevision>` when a producer thread must bump the same token).
///
/// R1270 §6.3 — carries an optional [`RevisionObserver`] installed **once** by
/// the embedder ([`set_observer`](Self::set_observer)): [`bump`](Self::bump)
/// fires it with the new value, so a change-driven consumer (the async
/// `scene/waitFor` waiter registry)
/// wakes on **one** token — the same one the OCC preview lifecycle guards —
/// no matter which site bumped it (a dispatched mutation, shell input, or an
/// external-data arrival). This is why external arrival must bump *this*
/// revision rather than a private counter: one scene, one version.
pub struct SceneRevision {
    counter: AtomicU64,
    /// Set once at wiring time; read lock-free on the bump hot path.
    observer: OnceLock<RevisionObserver>,
}

impl SceneRevision {
    /// Construct a fresh revision starting at `0`, with no observer. The
    /// first [`bump`](Self::bump) yields `1`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
            observer: OnceLock::new(),
        }
    }

    /// Read the current revision value.
    ///
    /// Uses `Acquire` ordering so reads observing this value also
    /// observe any prior `bump` on the same counter and the scene
    /// mutations it ordered behind.
    #[must_use]
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::Acquire)
    }

    /// Increment the revision and return the new value, then fire the
    /// installed [observer](Self::set_observer) (if any) with it.
    ///
    /// Uses `AcqRel` ordering so a subsequent [`current`](Self::current)
    /// on another thread is guaranteed to see this bump. The observer is
    /// invoked **after** the counter advance and read lock-free (an
    /// [`OnceLock`] get), so a consumer waking off the bump sees the new
    /// value; it runs on whatever thread called `bump` (the observer must be
    /// `Send + Sync` for exactly that reason).
    pub fn bump(&self) -> u64 {
        let new = self.counter.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(observer) = self.observer.get() {
            observer(new);
        }
        new
    }

    /// Install the wake [observer](RevisionObserver), fired by every future
    /// [`bump`](Self::bump). Idempotent-once: the first call wins and any
    /// later call is a no-op (the embedder installs exactly one wake seam at
    /// boot; there is no unset). Returns whether this call installed it.
    pub fn set_observer(&self, observer: impl Fn(u64) + Send + Sync + 'static) -> bool {
        self.observer.set(Box::new(observer)).is_ok()
    }

    /// Whether an [observer](Self::set_observer) is installed — for the
    /// embedder's boot wiring and tests.
    #[must_use]
    pub fn has_observer(&self) -> bool {
        self.observer.get().is_some()
    }
}

impl core::fmt::Debug for SceneRevision {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The observer closure is not `Debug`; report the counter + whether a
        // wake seam is installed (keeps `#[derive(Debug)]` working on holders).
        f.debug_struct("SceneRevision")
            .field("current", &self.current())
            .field("has_observer", &self.has_observer())
            .finish()
    }
}

impl Default for SceneRevision {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn new_starts_at_zero() {
        let rev = SceneRevision::new();
        assert_eq!(rev.current(), 0);
    }

    #[test]
    fn bump_returns_new_value() {
        let rev = SceneRevision::new();
        assert_eq!(rev.bump(), 1);
        assert_eq!(rev.bump(), 2);
        assert_eq!(rev.bump(), 3);
    }

    #[test]
    fn current_reflects_latest_bump() {
        let rev = SceneRevision::new();
        rev.bump();
        rev.bump();
        rev.bump();
        assert_eq!(rev.current(), 3);
    }

    #[test]
    fn default_starts_at_zero() {
        let rev = SceneRevision::default();
        assert_eq!(rev.current(), 0);
    }

    #[test]
    fn bump_fires_the_installed_observer_with_the_new_value() {
        use std::sync::Mutex;
        let rev = SceneRevision::new();
        assert!(!rev.has_observer(), "no observer by default");
        let seen: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let s = Arc::clone(&seen);
        assert!(rev.set_observer(move |n| s.lock().unwrap().push(n)));
        assert!(rev.has_observer());
        assert_eq!(rev.bump(), 1);
        assert_eq!(rev.bump(), 2);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![1, 2],
            "observer saw every new value"
        );
    }

    #[test]
    fn set_observer_is_install_once() {
        let rev = SceneRevision::new();
        assert!(rev.set_observer(|_| {}), "first install wins");
        assert!(!rev.set_observer(|_| {}), "a second install is a no-op");
    }

    #[test]
    fn no_observer_bump_is_side_effect_free() {
        let rev = SceneRevision::new();
        assert_eq!(
            rev.bump(),
            1,
            "a bump with no observer just advances the counter"
        );
    }

    #[test]
    fn observer_fires_from_the_bumping_thread() {
        use std::sync::Mutex;
        // A bump on a spawned thread fires the observer there — the
        // external-arrival path (a producer thread bumping the shared token).
        let rev = Arc::new(SceneRevision::new());
        let count = Arc::new(Mutex::new(0u64));
        let c = Arc::clone(&count);
        rev.set_observer(move |_| *c.lock().unwrap() += 1);
        let r = Arc::clone(&rev);
        thread::spawn(move || {
            r.bump();
        })
        .join()
        .unwrap();
        assert_eq!(
            *count.lock().unwrap(),
            1,
            "the observer fired on the producer thread"
        );
        assert_eq!(rev.current(), 1);
    }

    #[test]
    fn concurrent_bumps_are_unique_and_monotonic() {
        let rev = Arc::new(SceneRevision::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let r = Arc::clone(&rev);
            handles.push(thread::spawn(move || {
                let mut seen = Vec::with_capacity(8);
                for _ in 0..8 {
                    seen.push(r.bump());
                }
                seen
            }));
        }
        let mut all: Vec<u64> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();
        all.sort_unstable();
        // 64 bumps starting from 1: values 1..=64, all unique.
        assert_eq!(all.len(), 64);
        let mut expected: Vec<u64> = (1..=64).collect();
        expected.sort_unstable();
        assert_eq!(all, expected);
        assert_eq!(rev.current(), 64);
    }
}
