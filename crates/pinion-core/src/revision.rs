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

use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic version counter attached to the live scene (§5.34 R40.4).
///
/// Thread-safe — backed by a single [`AtomicU64`]. Cloning is *not*
/// implemented because the revision is a per-scene singleton; the
/// embedder owns one and shares `&SceneRevision` references to it.
#[derive(Debug)]
pub struct SceneRevision(AtomicU64);

impl SceneRevision {
    /// Construct a fresh revision starting at `0`. The first
    /// [`bump`](Self::bump) yields `1`.
    #[must_use]
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Read the current revision value.
    ///
    /// Uses `Acquire` ordering so reads observing this value also
    /// observe any prior `bump` on the same counter and the scene
    /// mutations it ordered behind.
    #[must_use]
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    /// Increment the revision and return the new value.
    ///
    /// Uses `AcqRel` ordering so a subsequent [`current`](Self::current)
    /// on another thread is guaranteed to see this bump.
    pub fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel) + 1
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
        let mut all: Vec<u64> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_unstable();
        // 64 bumps starting from 1: values 1..=64, all unique.
        assert_eq!(all.len(), 64);
        let mut expected: Vec<u64> = (1..=64).collect();
        expected.sort_unstable();
        assert_eq!(all, expected);
        assert_eq!(rev.current(), 64);
    }
}
