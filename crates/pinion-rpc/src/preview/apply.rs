//! `scene/apply_preview` typed dispatcher (§5.34 R40.6 / R40.9).
//!
//! Closes the preview lifecycle: re-reads the live [`SceneRevision`]
//! for an OCC check, extracts the proposal from the ledger, dispatches
//! into the variant's own [`Proposal::apply`] through an
//! [`ApplyContext`] (R40.9: bundle pattern so future variants can
//! gain side-effect targets beyond the scene), and bumps the revision
//! so any *other* still-active preview becomes detectably stale.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) lives in
//! [`crate::dispatch`]; this module exposes the typed dispatcher only.

use std::time::Instant;

use pinion_core::intent::Intent;
use pinion_core::{Scene, SceneRevision};

use super::{ApplyContext, ApplyError, PreviewId, PreviewLedger};

/// Successful outcome of [`apply_preview`].
///
/// `#[non_exhaustive]` so further side-effect channels (animation
/// registry receipts, effect ledger acknowledgements, …) can be
/// added non-breakingly per Bloch / Hyrum API-evolution.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyOutcome {
    /// Preview that was applied (now consumed; subsequent
    /// `apply_preview` / `cancel_preview` on the same id surface
    /// [`ApplyError::UnknownPreview`]).
    pub preview_id: PreviewId,
    /// `SceneRevision` value after the post-apply bump. Equals
    /// `revision.current()` upon return; surfaced here so the wire
    /// caller does not need a second RPC round-trip to read it.
    pub new_revision: u64,
    /// Intents emitted by the proposal during apply (§5.34 R40.9).
    /// Non-empty only when a variant explicitly emits — currently
    /// just [`super::TypedProposal::DispatchIntent`]; other variants
    /// (`SetSignal`, future `SetStyle`/`ReplaceView`) leave this empty.
    pub emitted_intents: Vec<Intent>,
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
///    proposal's [`crate::preview::Proposal::apply`] runs against a
///    fresh [`ApplyContext`] wrapping `scene`. The context's
///    [`emitted_intents`](ApplyContext::emitted_intents) accumulator
///    is drained into [`ApplyOutcome::emitted_intents`]. Variant-
///    specific runtime rejections (signal type mismatch, unknown
///    signal path, etc.) surface as [`ApplyError::ApplyRejected`]
///    carrying a short tag string.
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
    let mut ctx = ApplyContext::new(scene);
    proposal
        .apply(&mut ctx)
        .map_err(ApplyError::ApplyRejected)?;
    let new_revision = revision.bump();
    Ok(ApplyOutcome {
        preview_id: id,
        new_revision,
        emitted_intents: ctx.emitted_intents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{TypedProposal, propose_change};
    use pinion_core::Scene;
    use pinion_core::external::{CountedExternal, IntrospectValue};
    use pinion_core::scene::ExternalNode;

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
        let outcome = propose_change(
            &ledger,
            &revision,
            set_count_proposal(serde_json::json!(99)),
            None,
        )
        .unwrap();
        let applied = apply_preview(&mut scene, &revision, &ledger, outcome.preview_id).unwrap();
        assert_eq!(applied.preview_id, outcome.preview_id);
        assert_eq!(applied.new_revision, 1, "apply bumps revision exactly once");
        assert!(
            applied.emitted_intents.is_empty(),
            "SetSignal does not emit intents — only DispatchIntent does",
        );

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
        let outcome = propose_change(
            &ledger,
            &revision,
            set_count_proposal(serde_json::json!(1)),
            None,
        )
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
        let outcome = propose_change(
            &ledger,
            &revision,
            set_count_proposal(serde_json::json!(7)),
            None,
        )
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

    #[test]
    fn apply_preview_dispatch_intent_surfaces_intent_in_outcome() {
        // §5.34 R40.9 end-to-end: propose a DispatchIntent, apply,
        // observe the emitted intent in ApplyOutcome.
        let mut scene = counted_scene(0);
        let ledger = PreviewLedger::default();
        let revision = SceneRevision::default();
        let intent =
            pinion_core::intent::Intent::new_static("save_btn.click", IntrospectValue::Null);
        let outcome = propose_change(
            &ledger,
            &revision,
            TypedProposal::DispatchIntent {
                target_path: "/save_btn".to_string(),
                intent: intent.clone(),
            },
            None,
        )
        .unwrap();
        let applied = apply_preview(&mut scene, &revision, &ledger, outcome.preview_id).unwrap();
        assert_eq!(applied.emitted_intents.len(), 1);
        assert_eq!(applied.emitted_intents[0].tag_str(), "save_btn.click");
        assert_eq!(
            applied.new_revision, 1,
            "apply bumps revision even for non-scene side-effects"
        );
        assert!(ledger.is_empty(), "DispatchIntent apply consumes the entry");
    }
}
