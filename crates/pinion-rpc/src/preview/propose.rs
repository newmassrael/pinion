//! `scene/propose_change` typed dispatcher (§5.34 R40.5).
//!
//! Composes [`PreviewLedger::propose`] with [`SceneRevision::current`]
//! to capture the OCC token at propose time, store the proposal under
//! a fresh [`PreviewId`], and return both to the caller for round-trip
//! introspection.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) lives in
//! [`crate::dispatch`](fn@crate::dispatch); this module exposes the typed dispatcher only.

use std::time::{Duration, Instant};

use pinion_core::SceneRevision;

use super::{PreviewId, PreviewLedger, ProposeError, TypedProposal};

/// Successful outcome of [`propose_change`]. Carries both the fresh
/// handle and the base-revision snapshot captured at propose time;
/// the caller passes the latter back at `scene/apply_preview` time
/// (R40.6) so OCC conflict detection can fire on a moved scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposeOutcome {
    /// Stable handle the caller uses to reference the preview in
    /// subsequent `cancel_preview` / `list_previews` / `apply_preview`
    /// calls.
    pub preview_id: PreviewId,
    /// `SceneRevision` value at the moment the preview was inserted
    /// into the ledger. Equality with `revision.current()` at apply
    /// time is the OCC guard.
    pub base_revision: u64,
}

/// Capture the live [`SceneRevision`], box `proposal`, and insert
/// into `ledger`.
///
/// `ttl_hint` is forwarded verbatim to [`PreviewLedger::propose`],
/// which clamps it against `MAX_TTL` and falls back to `DEFAULT_TTL`
/// on `None`.
///
/// # Errors
///
/// Returns [`ProposeError::CapacityFull`] when the ledger is already
/// at capacity after its lazy sweep of expired entries.
pub fn propose_change(
    ledger: &PreviewLedger,
    revision: &SceneRevision,
    proposal: TypedProposal,
    ttl_hint: Option<Duration>,
) -> Result<ProposeOutcome, ProposeError> {
    let base = revision.current();
    let now = Instant::now();
    let preview_id = ledger.propose(base, Box::new(proposal), ttl_hint, now)?;
    Ok(ProposeOutcome {
        preview_id,
        base_revision: base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal_proposal() -> TypedProposal {
        TypedProposal::SetSignal {
            target_path: "/w/counter".to_string(),
            signal_path: "/w/counter/count".to_string(),
            value: serde_json::json!(7),
        }
    }

    #[test]
    fn propose_change_captures_base_revision() {
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        revision.bump();
        revision.bump();
        let outcome = propose_change(&ledger, &revision, signal_proposal(), None).unwrap();
        assert_eq!(outcome.base_revision, 2);
    }

    #[test]
    fn propose_change_returns_unique_ids() {
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        let a = propose_change(&ledger, &revision, signal_proposal(), None).unwrap();
        let b = propose_change(&ledger, &revision, signal_proposal(), None).unwrap();
        let c = propose_change(&ledger, &revision, signal_proposal(), None).unwrap();
        assert_ne!(a.preview_id, b.preview_id);
        assert_ne!(b.preview_id, c.preview_id);
        assert_eq!(ledger.len(), 3);
    }

    #[test]
    fn propose_change_surfaces_capacity_full() {
        let ledger =
            PreviewLedger::with_config(1, Duration::from_secs(60), Duration::from_secs(600));
        let revision = SceneRevision::default();
        propose_change(&ledger, &revision, signal_proposal(), None).unwrap();
        let err = propose_change(&ledger, &revision, signal_proposal(), None).unwrap_err();
        assert_eq!(err, ProposeError::CapacityFull { capacity: 1 });
    }
}
