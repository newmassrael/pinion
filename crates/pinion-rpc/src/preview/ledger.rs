//! [`PreviewLedger`] — the stateful core of §5.34's preview lifecycle.
//!
//! Issued [`PreviewId`]s, base-revision tokens for optimistic
//! concurrency control (Q2=C), TTL-clamped deadlines, capacity-bounded
//! storage, and the four mutating methods the R40.2+ RPC dispatch
//! handlers compose over (`propose`, `cancel`, `list`, `apply_extract`).

use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{ApplyError, PreviewId, Proposal, ProposeError};

/// Default per-preview TTL applied when the caller does not supply a
/// hint. 60 s is long enough for an AI agent to introspect a preview
/// (locate / bbox / screenshot) and decide whether to apply, short
/// enough that abandoned previews do not pin memory for an extended
/// session.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

/// Hard upper bound the server clamps every TTL hint against. Caps
/// pathological client requests and bounds ledger memory residency.
pub const MAX_TTL: Duration = Duration::from_secs(600);

/// Default concurrent-preview capacity. Sufficient for AI workflows
/// that fan out a handful of alternative scenarios; raise via
/// [`PreviewLedger::with_config`] when running with many agents.
pub const DEFAULT_CAPACITY: usize = 64;

/// Stateful preview lifecycle ledger.
///
/// Thread-safe: holds entries behind an [`RwLock`] (multi-reader for
/// [`list`], single-writer for [`propose`] / [`cancel`] /
/// [`apply_extract`] / [`sweep_expired`]) and generates ids through
/// an [`AtomicU64`] counter. Every method is non-async and returns
/// promptly; the lock is held only across the immediate map mutation.
///
/// [`list`]: PreviewLedger::list
/// [`propose`]: PreviewLedger::propose
/// [`cancel`]: PreviewLedger::cancel
/// [`apply_extract`]: PreviewLedger::apply_extract
/// [`sweep_expired`]: PreviewLedger::sweep_expired
#[derive(Debug)]
pub struct PreviewLedger {
    next_id: AtomicU64,
    entries: RwLock<BTreeMap<PreviewId, Entry>>,
    capacity: usize,
    default_ttl: Duration,
    max_ttl: Duration,
}

/// A single in-ledger preview (§5.34 OCC entry).
#[derive(Debug)]
pub struct Entry {
    /// Scene-revision token captured at propose time. Compared against
    /// the live revision at [`PreviewLedger::apply_extract`]; mismatch
    /// yields [`ApplyError::BaseRevisionConflict`].
    pub base_revision: u64,
    /// Absolute deadline; `created_at + clamped_ttl`. Absolute storage
    /// (rather than `remaining_duration`) avoids drift across long
    /// sweeps and survives suspend/resume cleanly.
    pub deadline: Instant,
    /// Creation timestamp. Kept for audit and for forward-compatible
    /// future eviction policies (LRU, age-weighted scoring).
    pub created_at: Instant,
    /// The typed change descriptor itself.
    pub proposal: Box<dyn Proposal>,
}

/// Read-only summary of a single ledger entry, returned by
/// [`PreviewLedger::list`].
#[derive(Debug, Clone)]
pub struct PreviewView {
    /// Stable handle assigned at propose time.
    pub id: PreviewId,
    /// Scene-revision token captured at propose time. Callers can
    /// compare against the live revision to predict whether
    /// `apply_preview` will succeed or yield
    /// [`ApplyError::BaseRevisionConflict`].
    pub base_revision: u64,
    /// `proposal.target_path()` materialized as owned `String` for
    /// transport.
    pub target_path: String,
    /// `proposal.affected_paths()` materialized.
    pub affected_paths: Vec<String>,
    /// Wall-clock creation timestamp (audit).
    pub created_at: Instant,
    /// Absolute deadline; the preview becomes invalid once the live
    /// `Instant` passes this value.
    pub deadline: Instant,
}

/// Outcome from a [`PreviewLedger::sweep_expired`] pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    /// Number of entries whose deadline had passed and which were
    /// therefore removed from the ledger.
    pub removed: usize,
    /// Number of entries still active after the sweep.
    pub remaining: usize,
}

impl Default for PreviewLedger {
    fn default() -> Self {
        Self::with_config(DEFAULT_CAPACITY, DEFAULT_TTL, MAX_TTL)
    }
}

impl PreviewLedger {
    /// Construct a ledger with the supplied capacity and TTL bounds.
    ///
    /// # Panics
    ///
    /// Panics when `capacity == 0` (a zero-capacity ledger could only
    /// ever reject) or when `default_ttl > max_ttl` (an unreachable
    /// default value).
    #[must_use]
    pub fn with_config(capacity: usize, default_ttl: Duration, max_ttl: Duration) -> Self {
        assert!(capacity > 0, "PreviewLedger capacity must be non-zero");
        assert!(
            default_ttl <= max_ttl,
            "PreviewLedger default_ttl must not exceed max_ttl"
        );
        Self {
            next_id: AtomicU64::new(1),
            entries: RwLock::new(BTreeMap::new()),
            capacity,
            default_ttl,
            max_ttl,
        }
    }

    /// Configured concurrent-preview capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of currently-stored entries (including any past their
    /// deadline that have not yet been swept — call
    /// [`sweep_expired`] for live-only counts).
    ///
    /// [`sweep_expired`]: PreviewLedger::sweep_expired
    ///
    /// # Panics
    ///
    /// Panics if the internal entries lock has been poisoned by a
    /// panic in another thread; that condition represents an
    /// unrecoverable invariant failure in the lifecycle layer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.read().expect("ledger lock poisoned").len()
    }

    /// `true` when [`len`] is zero.
    ///
    /// [`len`]: PreviewLedger::len
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Allocate a fresh [`PreviewId`] and insert a new entry.
    ///
    /// Conflict policy is §5.34 Q2=C **independent ledger**: multiple
    /// previews may anchor the same path; this method never rejects on
    /// path collision. The only rejection cause is capacity.
    ///
    /// As a memcache-style lazy eviction, expired entries are reaped
    /// before the capacity check so callers do not need to invoke
    /// [`sweep_expired`] manually on every write.
    ///
    /// `ttl_hint` is clamped to `[0, max_ttl]`; `None` resolves to
    /// `default_ttl`.
    ///
    /// # Errors
    ///
    /// Returns [`ProposeError::CapacityFull`] when, after the lazy
    /// sweep, the ledger still holds `capacity` entries.
    ///
    /// # Panics
    ///
    /// Panics if the internal entries lock has been poisoned by a
    /// panic in another thread.
    ///
    /// [`sweep_expired`]: PreviewLedger::sweep_expired
    pub fn propose(
        &self,
        base_revision: u64,
        proposal: Box<dyn Proposal>,
        ttl_hint: Option<Duration>,
        now: Instant,
    ) -> Result<PreviewId, ProposeError> {
        let mut entries = self.entries.write().expect("ledger lock poisoned");
        entries.retain(|_, entry| entry.deadline > now);
        if entries.len() >= self.capacity {
            return Err(ProposeError::CapacityFull {
                capacity: self.capacity,
            });
        }
        let ttl = ttl_hint.unwrap_or(self.default_ttl).min(self.max_ttl);
        let raw = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = PreviewId::from_raw(
            NonZeroU64::new(raw).expect("AtomicU64 monotonic counter started at 1"),
        );
        entries.insert(
            id,
            Entry {
                base_revision,
                deadline: now + ttl,
                created_at: now,
                proposal,
            },
        );
        Ok(id)
    }

    /// Remove the entry for `id` if present.
    ///
    /// Returns `true` when the entry was active and removed; `false`
    /// when the id is unknown or already gone. Idempotent — repeated
    /// calls on the same handle return `false` after the first.
    ///
    /// # Panics
    ///
    /// Panics if the internal entries lock has been poisoned by a
    /// panic in another thread.
    pub fn cancel(&self, id: PreviewId) -> bool {
        let mut entries = self.entries.write().expect("ledger lock poisoned");
        entries.remove(&id).is_some()
    }

    /// Snapshot every entry whose deadline has not yet passed.
    ///
    /// Iteration is in [`PreviewId`] order — i.e., creation order —
    /// deterministically. Past-deadline entries are filtered but not
    /// removed; call [`sweep_expired`] for that.
    ///
    /// # Panics
    ///
    /// Panics if the internal entries lock has been poisoned by a
    /// panic in another thread (which can occur if a stored
    /// [`Proposal`] implementation panics inside `target_path` /
    /// `affected_paths`; impls are required not to panic).
    ///
    /// [`sweep_expired`]: PreviewLedger::sweep_expired
    pub fn list(&self, now: Instant) -> Vec<PreviewView> {
        let entries = self.entries.read().expect("ledger lock poisoned");
        entries
            .iter()
            .filter(|(_, entry)| entry.deadline > now)
            .map(|(id, entry)| PreviewView {
                id: *id,
                base_revision: entry.base_revision,
                target_path: entry.proposal.target_path().to_owned(),
                affected_paths: entry.proposal.affected_paths(),
                created_at: entry.created_at,
                deadline: entry.deadline,
            })
            .collect()
    }

    /// Remove the entry for `id` and return its proposal — the
    /// caller (R40.5 `scene/apply_preview`) is responsible for
    /// effecting the change against the runtime.
    ///
    /// `current_scene_revision` is compared against the entry's
    /// captured `base_revision` to gate the OCC check per §5.34 Q2=C.
    /// `now` is checked against the entry's deadline to detect
    /// implicit expiry.
    ///
    /// # Errors
    ///
    /// * [`ApplyError::UnknownPreview`] — id not present.
    /// * [`ApplyError::Expired`] — entry past deadline; the entry is
    ///   removed as a side-effect of this call.
    /// * [`ApplyError::BaseRevisionConflict`] — entry's
    ///   `base_revision` differs from `current_scene_revision`; the
    ///   entry is **kept** so the caller can inspect it (via
    ///   `list_previews`) or cancel it explicitly.
    ///
    /// # Panics
    ///
    /// Panics if the internal entries lock has been poisoned by a
    /// panic in another thread.
    pub fn apply_extract(
        &self,
        id: PreviewId,
        current_scene_revision: u64,
        now: Instant,
    ) -> Result<Box<dyn Proposal>, ApplyError> {
        let mut entries = self.entries.write().expect("ledger lock poisoned");
        let (expired, conflict) = match entries.get(&id) {
            None => return Err(ApplyError::UnknownPreview),
            Some(entry) => (
                entry.deadline <= now,
                (entry.base_revision != current_scene_revision)
                    .then_some(entry.base_revision),
            ),
        };
        if expired {
            entries.remove(&id);
            return Err(ApplyError::Expired);
        }
        if let Some(expected) = conflict {
            return Err(ApplyError::BaseRevisionConflict {
                expected,
                actual: current_scene_revision,
            });
        }
        let entry = entries
            .remove(&id)
            .expect("entry present per earlier get(&id) match");
        Ok(entry.proposal)
    }

    /// Remove every entry whose deadline has passed.
    ///
    /// # Panics
    ///
    /// Panics if the internal entries lock has been poisoned by a
    /// panic in another thread.
    pub fn sweep_expired(&self, now: Instant) -> SweepReport {
        let mut entries = self.entries.write().expect("ledger lock poisoned");
        let before = entries.len();
        entries.retain(|_, entry| entry.deadline > now);
        let remaining = entries.len();
        SweepReport {
            removed: before - remaining,
            remaining,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestProposal {
        target: String,
        extra_affected: Vec<String>,
    }

    impl TestProposal {
        fn at(target: &str) -> Box<dyn Proposal> {
            Box::new(Self {
                target: target.to_owned(),
                extra_affected: Vec::new(),
            })
        }

        fn at_with_descendants(target: &str, descendants: &[&str]) -> Box<dyn Proposal> {
            Box::new(Self {
                target: target.to_owned(),
                extra_affected: descendants.iter().map(|s| (*s).to_owned()).collect(),
            })
        }
    }

    impl Proposal for TestProposal {
        fn target_path(&self) -> &str {
            &self.target
        }

        fn affected_paths(&self) -> Vec<String> {
            let mut paths = vec![self.target.clone()];
            paths.extend(self.extra_affected.iter().cloned());
            paths
        }

        fn apply(
            &self,
            _ctx: &mut crate::preview::ApplyContext<'_>,
        ) -> Result<(), String> {
            // The PreviewLedger unit tests exercise the lifecycle
            // primitives (propose / cancel / list / apply_extract);
            // `apply_preview` end-to-end coverage lives in
            // [`crate::preview::apply::tests`]. Here, a no-op apply
            // keeps the ledger-level tests independent of the runtime
            // side-effect layer.
            Ok(())
        }
    }

    fn t0() -> Instant {
        Instant::now()
    }

    fn ledger_with_ttl(ttl: Duration) -> PreviewLedger {
        PreviewLedger::with_config(8, ttl, ttl.max(MAX_TTL))
    }

    #[test]
    fn propose_returns_monotonic_ids_starting_at_one() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let a = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        let b = ledger.propose(0, TestProposal::at("/b"), None, now).unwrap();
        let c = ledger.propose(0, TestProposal::at("/c"), None, now).unwrap();
        assert_eq!(a.get(), 1);
        assert_eq!(b.get(), 2);
        assert_eq!(c.get(), 3);
    }

    #[test]
    fn propose_allows_multiple_entries_on_same_target_path() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let a = ledger.propose(0, TestProposal::at("/same"), None, now).unwrap();
        let b = ledger.propose(0, TestProposal::at("/same"), None, now).unwrap();
        let c = ledger.propose(0, TestProposal::at("/same"), None, now).unwrap();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(ledger.len(), 3);
    }

    #[test]
    fn propose_fails_with_capacity_full_at_limit() {
        let ledger = PreviewLedger::with_config(2, DEFAULT_TTL, MAX_TTL);
        let now = t0();
        ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        ledger.propose(0, TestProposal::at("/b"), None, now).unwrap();
        let err = ledger
            .propose(0, TestProposal::at("/c"), None, now)
            .unwrap_err();
        assert_eq!(err, ProposeError::CapacityFull { capacity: 2 });
    }

    #[test]
    fn propose_lazy_sweep_reclaims_expired_slots() {
        let ledger = PreviewLedger::with_config(2, Duration::from_secs(1), MAX_TTL);
        let now = t0();
        ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        ledger.propose(0, TestProposal::at("/b"), None, now).unwrap();
        let later = now + Duration::from_secs(2);
        // At capacity, but both entries are past deadline → lazy sweep reclaims.
        let c = ledger
            .propose(0, TestProposal::at("/c"), None, later)
            .unwrap();
        assert_eq!(c.get(), 3, "id counter still monotonic across sweeps");
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn propose_clamps_ttl_hint_above_max() {
        let ledger =
            PreviewLedger::with_config(8, Duration::from_secs(10), Duration::from_secs(30));
        let now = t0();
        let id = ledger
            .propose(0, TestProposal::at("/a"), Some(Duration::from_secs(9999)), now)
            .unwrap();
        let view = ledger
            .list(now)
            .into_iter()
            .find(|v| v.id == id)
            .expect("entry visible immediately");
        assert_eq!(view.deadline - now, Duration::from_secs(30));
    }

    #[test]
    fn propose_uses_default_ttl_when_hint_absent() {
        let ledger = ledger_with_ttl(Duration::from_secs(7));
        let now = t0();
        let id = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        let view = ledger.list(now).into_iter().find(|v| v.id == id).unwrap();
        assert_eq!(view.deadline - now, Duration::from_secs(7));
    }

    #[test]
    fn cancel_removes_active_entry_and_is_idempotent() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let id = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        assert!(ledger.cancel(id));
        assert!(!ledger.cancel(id), "second cancel returns false");
        assert!(ledger.is_empty());
    }

    #[test]
    fn cancel_unknown_id_returns_false() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let id = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        let _ = ledger.apply_extract(id, 0, now).unwrap();
        // id is now consumed.
        assert!(!ledger.cancel(id));
    }

    #[test]
    fn list_returns_entries_in_id_order_and_filters_expired() {
        let ledger = PreviewLedger::with_config(8, Duration::from_secs(5), MAX_TTL);
        let now = t0();
        let a = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        let b = ledger.propose(0, TestProposal::at("/b"), None, now).unwrap();
        let c = ledger
            .propose(0, TestProposal::at("/c"), Some(Duration::from_secs(60)), now)
            .unwrap();
        let later = now + Duration::from_secs(10);
        let view = ledger.list(later);
        assert_eq!(view.len(), 1, "/a and /b expired, /c kept");
        assert_eq!(view[0].id, c);
        assert!(view.iter().all(|v| v.id != a && v.id != b));
    }

    #[test]
    fn list_surfaces_affected_paths_from_proposal() {
        let ledger = PreviewLedger::default();
        let now = t0();
        ledger
            .propose(
                0,
                TestProposal::at_with_descendants("/root", &["/root/a", "/root/b"]),
                None,
                now,
            )
            .unwrap();
        let view = ledger.list(now);
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].target_path, "/root");
        assert_eq!(
            view[0].affected_paths,
            vec!["/root".to_owned(), "/root/a".to_owned(), "/root/b".to_owned()]
        );
    }

    #[test]
    fn apply_extract_returns_proposal_when_revision_matches() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let id = ledger
            .propose(7, TestProposal::at("/target"), None, now)
            .unwrap();
        let proposal = ledger.apply_extract(id, 7, now).unwrap();
        assert_eq!(proposal.target_path(), "/target");
        assert!(ledger.is_empty(), "successful apply removes the entry");
    }

    #[test]
    fn apply_extract_unknown_returns_unknown_preview() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let id = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        ledger.cancel(id);
        let err = ledger.apply_extract(id, 0, now).unwrap_err();
        assert_eq!(err, ApplyError::UnknownPreview);
    }

    #[test]
    fn apply_extract_expired_returns_expired_and_removes_entry() {
        let ledger = PreviewLedger::with_config(8, Duration::from_secs(1), MAX_TTL);
        let now = t0();
        let id = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        let later = now + Duration::from_secs(2);
        let err = ledger.apply_extract(id, 0, later).unwrap_err();
        assert_eq!(err, ApplyError::Expired);
        assert!(ledger.is_empty(), "expired apply removes the entry");
    }

    #[test]
    fn apply_extract_revision_mismatch_keeps_entry() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let id = ledger
            .propose(3, TestProposal::at("/target"), None, now)
            .unwrap();
        let err = ledger.apply_extract(id, 5, now).unwrap_err();
        assert_eq!(
            err,
            ApplyError::BaseRevisionConflict {
                expected: 3,
                actual: 5,
            }
        );
        assert_eq!(ledger.len(), 1, "conflict does not remove the entry");
        // Caller can still cancel it explicitly.
        assert!(ledger.cancel(id));
    }

    #[test]
    fn apply_extract_cannot_be_called_twice() {
        let ledger = PreviewLedger::default();
        let now = t0();
        let id = ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        let _ = ledger.apply_extract(id, 0, now).unwrap();
        let err = ledger.apply_extract(id, 0, now).unwrap_err();
        assert_eq!(err, ApplyError::UnknownPreview);
    }

    #[test]
    fn sweep_expired_removes_only_past_deadline_entries() {
        let ledger = PreviewLedger::with_config(8, Duration::from_secs(5), MAX_TTL);
        let now = t0();
        ledger.propose(0, TestProposal::at("/a"), None, now).unwrap();
        ledger.propose(0, TestProposal::at("/b"), None, now).unwrap();
        ledger
            .propose(0, TestProposal::at("/c"), Some(Duration::from_secs(60)), now)
            .unwrap();
        let report = ledger.sweep_expired(now + Duration::from_secs(10));
        assert_eq!(
            report,
            SweepReport {
                removed: 2,
                remaining: 1,
            }
        );
    }

    #[test]
    #[should_panic(expected = "capacity must be non-zero")]
    fn with_config_panics_on_zero_capacity() {
        let _ = PreviewLedger::with_config(0, DEFAULT_TTL, MAX_TTL);
    }

    #[test]
    #[should_panic(expected = "default_ttl must not exceed max_ttl")]
    fn with_config_panics_when_default_ttl_exceeds_max() {
        let _ = PreviewLedger::with_config(8, Duration::from_secs(60), Duration::from_secs(30));
    }

    #[test]
    fn ids_remain_unique_under_concurrent_propose() {
        use std::sync::Arc;
        use std::thread;

        let ledger = Arc::new(PreviewLedger::with_config(256, DEFAULT_TTL, MAX_TTL));
        let mut handles = Vec::new();
        for thread_idx in 0..8 {
            let ledger = Arc::clone(&ledger);
            handles.push(thread::spawn(move || {
                let now = t0();
                let mut ids = Vec::with_capacity(8);
                for sub in 0..8 {
                    let id = ledger
                        .propose(
                            0,
                            TestProposal::at(&format!("/t{thread_idx}/s{sub}")),
                            None,
                            now,
                        )
                        .unwrap();
                    ids.push(id);
                }
                ids
            }));
        }
        let mut seen = std::collections::HashSet::new();
        for handle in handles {
            for id in handle.join().unwrap() {
                assert!(seen.insert(id), "id {id} issued twice");
            }
        }
        assert_eq!(seen.len(), 64);
    }
}
