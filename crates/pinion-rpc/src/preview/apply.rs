//! `scene/apply_preview` typed dispatcher (§5.34 R40.6).
//!
//! Closes the preview lifecycle: re-reads the live [`SceneRevision`]
//! for an OCC check, extracts the proposal from the ledger, dispatches
//! into the variant's own [`Proposal::apply`], and bumps the revision
//! so any *other* still-active preview becomes detectably stale.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) lives in
//! [`crate::dispatch`]; this module exposes the typed dispatcher only.

use std::time::Instant;

use pinion_core::{Scene, SceneRevision};

use super::{ApplyError, PreviewId, PreviewLedger};

/// Successful outcome of [`apply_preview`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// Preview that was applied (now consumed; subsequent
    /// `apply_preview` / `cancel_preview` on the same id surface
    /// [`ApplyError::UnknownPreview`]).
    pub preview_id: PreviewId,
    /// `SceneRevision` value after the post-apply bump. Equals
    /// `revision.current()` upon return; surfaced here so the wire
    /// caller does not need a second RPC round-trip to read it.
    pub new_revision: u64,
}

/// Apply a stored preview against `scene`, gated by OCC against
/// `revision`.
///
/// Sequence:
/// 1. Read `revision.current()` and pass it as the
///    `current_scene_revision` to [`PreviewLedger::apply_extract`],
///    which compares it against the entry's captured
///    `base_revision`. On mismatch the entry stays put and
///    [`ApplyError::BaseRevisionConflict`] is returned.
/// 2. On match, the entry is removed from the ledger and its
///    proposal's [`crate::preview::Proposal::apply`] runs against
///    `scene`. Variant-specific runtime rejections (signal type
///    mismatch, unknown signal path, etc.) surface as
///    [`ApplyError::ApplyRejected`] carrying a short tag string.
/// 3. On a successful side-effect, [`SceneRevision::bump`] runs so
///    any *other* preview with the now-stale `base_revision` will
///    detect the conflict on its own apply call.
///
/// # Errors
///
/// See [`ApplyError`]. Specifically:
/// * [`ApplyError::UnknownPreview`] — id unknown.
/// * [`ApplyError::Expired`] — deadline passed (entry removed).
/// * [`ApplyError::BaseRevisionConflict`] — OCC mismatch (entry
///   retained for inspection / explicit cancel).
/// * [`ApplyError::ApplyRejected`] — variant-specific runtime
///   rejection (entry already consumed; caller must re-propose).
pub fn apply_preview(
    scene: &mut Scene,
    revision: &SceneRevision,
    ledger: &PreviewLedger,
    id: PreviewId,
) -> Result<ApplyOutcome, ApplyError> {
    let current = revision.current();
    let now = Instant::now();
    let proposal = ledger.apply_extract(id, current, now)?;
    proposal.apply(scene).map_err(ApplyError::ApplyRejected)?;
    let new_revision = revision.bump();
    Ok(ApplyOutcome {
        preview_id: id,
        new_revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{TypedProposal, propose_change};
    use pinion_core::external::{CountedExternal, IntrospectValue};
    use pinion_core::scene::ExternalNode;
    use pinion_core::Scene;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    fn set_count_proposal(value: serde_json::Value) -> TypedProposal {
        TypedProposal::SetSignal {
            target_path: "/external/count".to_string(),
            signal_path: "/external/count".to_string(),
            value,
        }
    }

    #[test]
    fn apply_preview_writes_signal_and_bumps_revision() {
        let mut scene = counted_scene(0);
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        let outcome =
            propose_change(&ledger, &revision, set_count_proposal(serde_json::json!(99)), None)
                .unwrap();
        let applied = apply_preview(
            &mut scene,
            &revision,
            &ledger,
            outcome.preview_id,
        )
        .unwrap();
        assert_eq!(applied.preview_id, outcome.preview_id);
        assert_eq!(applied.new_revision, 1, "apply bumps revision exactly once");

        // The CountedExternal slot now holds the proposed value.
        let observed = crate::query::query(&scene, "/external/count").unwrap();
        assert_eq!(observed, IntrospectValue::Int(99));
        assert!(ledger.is_empty(), "apply consumes the entry");
    }

    #[test]
    fn apply_preview_unknown_id_yields_unknown_preview() {
        let mut scene = counted_scene(0);
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        let outcome =
            propose_change(&ledger, &revision, set_count_proposal(serde_json::json!(1)), None)
                .unwrap();
        // Cancel it manually so the id becomes unknown.
        assert!(crate::preview::cancel_preview(&ledger, outcome.preview_id));
        let err = apply_preview(&mut scene, &revision, &ledger, outcome.preview_id).unwrap_err();
        assert_eq!(err, ApplyError::UnknownPreview);
    }

    #[test]
    fn apply_preview_revision_conflict_keeps_entry_and_does_not_bump() {
        let mut scene = counted_scene(0);
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        let outcome =
            propose_change(&ledger, &revision, set_count_proposal(serde_json::json!(7)), None)
                .unwrap();
        // Scene mutates underneath the preview.
        revision.bump();
        let err = apply_preview(&mut scene, &revision, &ledger, outcome.preview_id).unwrap_err();
        assert!(
            matches!(
                err,
                ApplyError::BaseRevisionConflict {
                    expected: 0,
                    actual: 1
                }
            ),
            "unexpected error: {err:?}"
        );
        // Entry retained — caller can still cancel or inspect.
        assert_eq!(ledger.len(), 1);
        // Revision did not advance further.
        assert_eq!(revision.current(), 1);
    }

    #[test]
    fn apply_preview_type_mismatch_yields_apply_rejected_and_consumes() {
        let mut scene = counted_scene(0);
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        // CountedExternal::count is Int; a bool write trips
        // RewindError::Intervene(TypeMismatch).
        let outcome = propose_change(
            &ledger,
            &revision,
            set_count_proposal(serde_json::json!(true)),
            None,
        )
        .unwrap();
        let err = apply_preview(&mut scene, &revision, &ledger, outcome.preview_id).unwrap_err();
        assert_eq!(err, ApplyError::ApplyRejected("Intervene".to_string()));
        // Failed apply still consumes the entry — caller re-proposes.
        assert!(ledger.is_empty());
    }

    #[test]
    fn apply_preview_unknown_signal_path_yields_apply_rejected() {
        let mut scene = counted_scene(0);
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        let proposal = TypedProposal::SetSignal {
            target_path: "/external/missing".to_string(),
            signal_path: "/external/missing".to_string(),
            value: serde_json::json!(1),
        };
        let outcome = propose_change(&ledger, &revision, proposal, None).unwrap();
        let err = apply_preview(&mut scene, &revision, &ledger, outcome.preview_id).unwrap_err();
        assert_eq!(err, ApplyError::ApplyRejected("Intervene".to_string()));
    }
}
