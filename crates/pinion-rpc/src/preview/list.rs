//! `scene/list_previews` typed dispatcher (§5.34 R40.3).
//!
//! Read-only snapshot of every active preview entry. Transport
//! (JSON-RPC 2.0 framing per §5.7) lives in [`crate::dispatch`]; this
//! module exposes the typed dispatcher only so the same logic is
//! reusable from non-JSON-RPC carriers.

use std::time::Instant;

use super::{PreviewLedger, PreviewView};

/// Snapshot every entry in `ledger` whose deadline has not yet passed.
///
/// Ordering is deterministic: entries appear in [`super::PreviewId`]
/// order, which is also creation order. Past-deadline entries are
/// filtered out without removing them — callers wishing to also
/// reclaim memory should invoke [`PreviewLedger::sweep_expired`].
///
/// Per §5.34 the operation is total: no error surface. An empty
/// ledger returns an empty `Vec`.
#[must_use]
pub fn list_previews(ledger: &PreviewLedger, now: Instant) -> Vec<PreviewView> {
    ledger.list(now)
}
