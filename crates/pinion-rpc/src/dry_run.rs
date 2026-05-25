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
//!
//! R666 §5.34 — scene-path syntax mirrors [`crate::invoke`] /
//! [`crate::rewind`]: `/external/<slot>` for the primary External
//! (v0), `/<tag>/external/<slot>` for tagged scene-tree walk (R42),
//! composite tags (`todo_toggle#1`) pass through verbatim so R55.D.5
//! `create_extra_externals` siblings can be dry-run-probed without
//! per-binding RPC plumbing.

use pinion_core::external::{InterveneError, IntrospectValue};
use pinion_core::{Scene, SimulationGuard};

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
    // R649 §5.23 R27 — wrap the entire dry_run scope so any Effect
    // observing Signals indirectly mutated by `intervene` (typically
    // through `SignalExternal` or binding-internal reactive chains)
    // skips its closure body for the duration. Owner.snapshot/restore
    // is NOT called here (single-write, External rollback is symmetric)
    // — Effect suppression alone keeps side effects from landing twice.
    let _sim_guard = SimulationGuard::enter();
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // R666 §5.34 — split at the `/external/` separator (mirror of
    // [`crate::rewind`] / [`crate::invoke`]). Scene segments + owned
    // introspect path so the two mutable borrows below can each open
    // a fresh `lookup_path_mut` walk.
    let (scene_segments, introspect_path) = path::split_at_external(resolved.scene_path)
        .map(|(segs, intro)| (segs, intro.to_string()))
        .ok_or(DryRunError::UnsupportedPath)?;

    // Phase 1: save the current value and apply the hypothetical write.
    // Constrained to a scope so the &mut borrow on `scene` ends before
    // the snapshot phase needs a shared borrow.
    let saved = {
        // R666 §5.34 — walk by tag/index then descend to the
        // addressed scene's primary External (R55.D.5 multi-widget
        // shape).
        let target = scene
            .lookup_path_mut(&scene_segments)
            .ok_or(DryRunError::NoExternalAtPath)?;
        let node = target
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
        // Same walk as phase 1 — phase 1 already validated the path,
        // so the `ok_or` arms are unreachable unless the External
        // mutated its own scene topology mid-flight.
        let target = scene
            .lookup_path_mut(&scene_segments)
            .ok_or(DryRunError::NoExternalAtPath)?;
        let node = target
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

    // ---- R666 §5.34 v1: multi-External addressing ----

    fn container_with_two_counted_siblings(
        primary_tag: &'static str,
        primary_count: i64,
        extra_tag: &'static str,
        extra_count: i64,
    ) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let primary = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(primary_count))).with_tag(primary_tag),
        );
        let extra = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(extra_count))).with_tag(extra_tag),
        );
        let mut c = ContainerNode::new(vec![primary, extra]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r666_dry_run_extra_external_by_composite_tag_rolls_back() {
        // Hypothetical write on the composite-tagged extra sibling;
        // post-call, both siblings retain their original values.
        let mut scene = container_with_two_counted_siblings(
            "todo_list", 100, "todo_toggle#1", 0,
        );
        let snap = dry_run(
            &mut scene,
            "/todo_toggle#1/external/count",
            IntrospectValue::Int(999),
        )
        .unwrap();

        // Snapshot captures the hypothetical 999 on the extra.
        match snap {
            SnapshotNode::Container(ref c) => {
                // The container snapshot holds both siblings; the
                // extra (second child) should show the hypothetical
                // value, the primary (first) keeps its original.
                let extra = c.children.get(1).expect("extra child present");
                if let SnapshotNode::External(ExternalSnapshot {
                    introspect: Some(fields),
                    ..
                }) = extra
                {
                    assert_eq!(fields[0].1, IntrospectValue::Int(999));
                } else {
                    panic!("expected External snapshot for extra, got {extra:?}");
                }
            }
            other => panic!("expected Container snapshot, got {other:?}"),
        }

        // Post-call: original values preserved (rollback).
        let primary = scene
            .find_external_with_tag("todo_list")
            .expect("primary present");
        let extra = scene
            .find_external_with_tag("todo_toggle#1")
            .expect("extra present");
        assert_eq!(
            primary.handle.introspect().unwrap().query("count"),
            Some(IntrospectValue::Int(100)),
        );
        assert_eq!(
            extra.handle.introspect().unwrap().query("count"),
            Some(IntrospectValue::Int(0)),
        );
    }

    #[test]
    fn r666_dry_run_unknown_segment_is_no_external() {
        let mut scene = container_with_two_counted_siblings(
            "primary", 0, "extra", 0,
        );
        let err = dry_run(
            &mut scene,
            "/ghost/external/count",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, DryRunError::NoExternalAtPath);
    }
}
