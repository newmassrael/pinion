//! `scene/dry_run` RPC method dispatch (§5.12 method 3 of 7, R16 slice 14).
//!
//! v0 lives at the **External introspect level** rather than the
//! §5.8-ratified SCE engine-level hook: the engine-side step-intercept
//! is a separate slice gated on pinion-runtime wiring. Today we provide
//! equivalent *test-and-rollback* semantics over the §5.15 item 8
//! surface:
//!
//!   1. `query(path)` → save the current value.
//!   2. `intervene(path, value)` → apply the hypothetical write.
//!   3. [`crate::snapshot`] the scene → capture the resulting shape.
//!   4. `intervene(path, saved)` → roll back to the original state.
//!   5. Return the snapshot from step 3.
//!
//! The scene observed by the caller after `dry_run` is identical to
//! the pre-call scene (modulo benign re-allocation inside External
//! implementations). Rollback failure at step 4 surfaces as
//! [`DryRunError::RollbackFailed`] — that path indicates an External
//! invariant violation, not user error.

use pinion_core::external::{InterveneError, IntrospectValue};
use pinion_core::Scene;

use crate::path::{self, PathError};
use crate::snapshot::{snapshot, SnapshotError, SnapshotNode};

/// Reasons [`dry_run`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// External did not opt in to §5.15 item 8 introspection.
    IntrospectionOptedOut,
    /// Could not save the slot's current value (path not in schema).
    /// No mutation occurred.
    InitialQueryFailed,
    /// `intervene` rejected the hypothetical write (type mismatch,
    /// unknown path, or read-only). No mutation occurred.
    Intervene(InterveneError),
    /// Rollback `intervene` failed — invariant violation; caller's
    /// scene is now in an indeterminate state.
    RollbackFailed,
    /// Internal snapshot step failed (window-prefix / path parsing on
    /// the same `raw_path`). Should not normally fire since the prefix
    /// already validated.
    SnapshotFailed,
}

impl From<PathError> for DryRunError {
    fn from(err: PathError) -> Self {
        DryRunError::Path(err)
    }
}

/// Hypothetically write `value` at the addressed slot, snapshot the
/// scene, then roll back. The returned snapshot reflects the state as
/// it *would* be after the write.
///
/// # Errors
///
/// See [`DryRunError`] for the failure modes.
pub fn dry_run(
    scene: &mut Scene,
    raw_path: &str,
    value: IntrospectValue,
) -> Result<SnapshotNode, DryRunError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    let introspect_path = resolved
        .scene_path
        .strip_prefix("/external/")
        .ok_or(DryRunError::UnsupportedPath)?
        .to_string();

    // Phase 1: save the current value and apply the hypothetical write.
    // Constrained to a scope so the &mut borrow on `scene` ends before
    // the snapshot phase needs a shared borrow.
    let saved = {
        // (R55.D.5 §5.45) Descend to the substrate's primary External
        // — single-widget binding returns `self` immediately, multi-
        // widget binding (`create_extra_externals` non-empty) descends
        // through the wrapper `Container` to the first child External.
        let node = scene
            .primary_external_mut()
            .ok_or(DryRunError::NoExternalAtPath)?;
        let intro = node
            .handle
            .introspect_mut()
            .ok_or(DryRunError::IntrospectionOptedOut)?;

        let saved = intro
            .query(&introspect_path)
            .ok_or(DryRunError::InitialQueryFailed)?;

        intro
            .intervene(&introspect_path, value)
            .map_err(DryRunError::Intervene)?;
        saved
    };

    // Phase 2: capture the snapshot of the hypothetical state.
    let snap_result = snapshot(scene, "");

    // Phase 3: roll back to the saved value regardless of snapshot
    // outcome, so the caller's scene is never left mutated.
    let rollback_result = {
        // (R55.D.5 §5.45) Same primary-External descent as phase 1.
        // Unreachable error: phase 1 already located the External and
        // we did not swap the scene out from under us.
        let node = scene
            .primary_external_mut()
            .ok_or(DryRunError::NoExternalAtPath)?;
        let intro = node
            .handle
            .introspect_mut()
            .ok_or(DryRunError::IntrospectionOptedOut)?;
        intro.intervene(&introspect_path, saved)
    };

    if rollback_result.is_err() {
        return Err(DryRunError::RollbackFailed);
    }

    snap_result.map_err(|_e: SnapshotError| DryRunError::SnapshotFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};
    use pinion_core::Color;

    use crate::query::query;
    use crate::snapshot::ExternalSnapshot;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn dry_run_returns_hypothetical_snapshot_and_rolls_back() {
        let mut scene = counted_scene(7);
        let snap = dry_run(&mut scene, "/external/count", IntrospectValue::Int(999)).unwrap();

        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
            }) => assert_eq!(fields[0].1, IntrospectValue::Int(999)),
            other => panic!("expected External snapshot with hypothetical value, got {other:?}"),
        }

        // Post-call: original value preserved.
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(7),
        );
    }

    #[test]
    fn dry_run_with_window_prefix_short_circuits() {
        let mut scene = counted_scene(0);
        dry_run(
            &mut scene,
            "/window[main]/external/count",
            IntrospectValue::Int(5),
        )
        .unwrap();
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(0),
        );
    }

    #[test]
    fn stub_external_opts_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = dry_run(&mut scene, "/external/anything", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, DryRunError::IntrospectionOptedOut);
    }

    #[test]
    fn box_at_root_rejected() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = dry_run(&mut scene, "/external/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, DryRunError::NoExternalAtPath);
    }

    #[test]
    fn type_mismatch_does_not_mutate() {
        let mut scene = counted_scene(11);
        let err =
            dry_run(&mut scene, "/external/count", IntrospectValue::Bool(true)).unwrap_err();
        assert_eq!(err, DryRunError::Intervene(InterveneError::TypeMismatch));
        // Scene state unchanged.
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(11),
        );
    }

    #[test]
    fn unknown_introspect_path_fails_initial_query() {
        let mut scene = counted_scene(0);
        let err = dry_run(&mut scene, "/external/ghost", IntrospectValue::Int(1)).unwrap_err();
        assert_eq!(err, DryRunError::InitialQueryFailed);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let mut scene = counted_scene(0);
        let err = dry_run(
            &mut scene,
            "/window[main/external/count",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, DryRunError::Path(PathError::MalformedPrefix));
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let mut scene = counted_scene(0);
        let err = dry_run(&mut scene, "/some/other", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, DryRunError::UnsupportedPath);
    }
}
