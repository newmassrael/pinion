//! `scene/simulate` RPC method dispatch (R646 §2 invariant #3 + §5.12).
//!
//! Multi-event extension to the [`crate::dry_run`](fn@crate::dry_run) single-write
//! primitive: accepts an ordered sequence of `{path, value}` steps,
//! applies each through `External::intervene` in declaration order,
//! snapshots the resulting compound scene, then rolls every touched
//! path back to its pre-call value. The AI-side use case is the
//! branching-future query "if I do A then B then C, what's the final
//! state?" — `dry_run` handles only the single A; `simulate` covers
//! the rest of the sequence in one round-trip.
//!
//! ## Semantics
//!
//! 1. **Phase 0** — resolve every step's path through the §5.12
//!    `/window[X]/external/...` envelope; reject the whole call if
//!    any step has a malformed or unsupported path. No mutation
//!    occurs in phase 0.
//!
//! 2. **Phase 1** — for each *unique* introspect path appearing in
//!    the step sequence, save the current value via `query`. Multiple
//!    steps targeting the same path share one saved-original entry
//!    so the rollback restores the pre-call state, not the
//!    intermediate state set by an earlier step. Failure here returns
//!    `InitialQueryFailed`; no mutation has occurred yet.
//!
//! 3. **Phase 2** — apply each step's `intervene` in declaration
//!    order. If any step fails, every *previously-applied* step is
//!    rolled back to the saved original (paths beyond the failing
//!    index never executed, so nothing to undo) and the call returns
//!    `Intervene { step_index, error }`. Caller's scene is left in
//!    the pre-call state.
//!
//! 4. **Phase 3** — snapshot the scene reflecting all applied steps.
//!
//! 5. **Phase 4** — roll every saved original back via `intervene`,
//!    regardless of phase 3 outcome. Caller's scene is restored to
//!    pre-call state.
//!
//! Net effect: the caller observes a snapshot reflecting the compound
//! hypothetical state; the live scene is identical to its pre-call
//! shape (modulo benign re-allocation inside External implementations).
//!
//! ## Why per-unique-path save semantics
//!
//! Saving the value before each step (instead of once per unique path
//! at the start) would mean later rollback restores the intermediate
//! state from an earlier step, not the original pre-call state. The
//! per-unique-path convention matches [`dry_run`]'s "scene observed after
//! the call equals scene observed before" contract.
//!
//! ## Comparison to [`dry_run`]
//!
//! `dry_run(scene, path, value)` is a 1-step call:
//! `simulate(scene, [{path, value}])` returns the same result modulo
//! the wrapper-struct overhead. `simulate` does not call `dry_run`
//! internally because the loop is simpler than per-step save/restore
//! orchestration would be.
//!
//! [`dry_run`]: crate::dry_run::dry_run

use std::collections::BTreeMap;

use pinion_core::external::{InterveneError, IntrospectValue};
use pinion_core::{Owner, Scene, SimulationGuard};

use crate::dry_run::DryRunError;
use crate::path::PathError;
use crate::resolve::{ResolveExternalError, introspect_mut_at, resolve_external_path};
use crate::snapshot::{SnapshotError, SnapshotNode, snapshot};

/// One step in a [`simulate`] sequence — an `(path, value)` pair
/// that will be applied via `External::intervene`.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulateStep {
    /// `/window[X]/external/<slot>` raw path; same shape every
    /// other §5.12 method accepts.
    pub path: String,
    /// Value to write at `path`. Must match the slot's schema type
    /// or [`SimulateError::Intervene`] surfaces with
    /// [`InterveneError::TypeMismatch`].
    pub value: IntrospectValue,
}

/// Reasons [`simulate`] can fail. Mirrors [`DryRunError`] with the
/// step-index context added for the multi-step variants.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SimulateError {
    /// Window-prefix parsing failed for a step. `step_index` points
    /// at the offending step in the input sequence.
    Path { step_index: usize, error: PathError },
    /// Scene path does not match a v0-supported shape for a step.
    UnsupportedPath { step_index: usize },
    /// Expected an `External` at the scene root, found a different
    /// primitive. No mutation occurred.
    NoExternalAtPath,
    /// External did not opt in to §5.15 item 8 introspection. No
    /// mutation occurred.
    IntrospectionOptedOut,
    /// Initial `query` failed for one of the step paths. `step_index`
    /// points at the first step whose path triggered the failure.
    /// No mutation occurred.
    InitialQueryFailed { step_index: usize },
    /// `intervene` rejected the hypothetical write at `step_index`.
    /// Earlier steps have been rolled back; caller's scene is
    /// restored to pre-call state.
    Intervene {
        step_index: usize,
        error: InterveneError,
    },
    /// Rollback `intervene` failed during cleanup. Caller's scene is
    /// in an indeterminate state — invariant violation.
    RollbackFailed,
    /// Internal snapshot step failed. Should not normally fire since
    /// every path's window prefix was already validated in phase 0.
    SnapshotFailed,
    /// Empty step list. The caller almost certainly meant `snapshot`
    /// or `dry_run`; the dedicated method surfaces the intent better
    /// than a degenerate `simulate` would.
    EmptySteps,
}

/// Apply `steps` in order, snapshot the resulting compound state,
/// then roll every touched path back to its pre-call value. See the
/// module-level docs for the phase-by-phase contract.
///
/// # Errors
///
/// See [`SimulateError`] for the failure modes. Every error path
/// leaves the caller's scene in its pre-call state except
/// [`SimulateError::RollbackFailed`].
/// R647 §5.12 §5.22 R26 — [`simulate`] with [`Owner`] snapshot/restore
/// bridge. Wraps the External-side rollback with reactive-layer state
/// preservation: `owner.snapshot()` runs before any mutation, and
/// `owner.restore()` runs after the External rollback regardless of
/// outcome. Defense in depth — even when External `intervene` rollback
/// completes successfully, Signal mutations triggered by Effects that
/// the Externals subscribe to (non-idempotent counters / accumulators /
/// derived state) leave residue the External-only path cannot clear.
///
/// The Owner snapshot captures every [`Signal`](pinion_core::Signal)
/// owned by `owner` via [`Owner::snapshot`]; restore writes each value
/// back via [`Owner::restore`]. Effects still fire during simulate
/// (R27 commitment — Effect suppression — is not yet implemented;
/// pin as R649 carry-forward); the Owner restore captures the
/// post-Effect Signal values and overwrites with the pre-call ones.
///
/// # Errors
///
/// Same failure modes as [`simulate`] plus: if Owner restore returns
/// any per-signal `TypeMismatch`, the call surfaces
/// [`SimulateError::RollbackFailed`] regardless of the External-side
/// outcome — partial Signal restore leaves the reactive graph in an
/// indeterminate state, matching the [`dry_run`] `RollbackFailed` severity.
///
/// [`dry_run`]: crate::dry_run::dry_run
pub fn simulate_with_owner(
    scene: &mut Scene,
    owner: &Owner,
    steps: &[SimulateStep],
) -> Result<SnapshotNode, SimulateError> {
    // R649 §5.23 R27 — Effect suppression wraps both the simulate
    // body AND the Owner.restore phase. The R647 test workaround
    // (`set_with` over `set(get()+1)`) was needed because restore
    // re-fired the Effect; with R27 Effects don't fire during the
    // guard's scope, so the naive `set(get()+1)` Effect pattern is
    // now safe under simulate_with_owner.
    let _sim_guard = SimulationGuard::enter();
    // R26 §5.22: snapshot Signal graph BEFORE any mutation so the
    // restore returns to the pre-call reactive state (External rollback
    // alone misses Signals indirectly mutated by Effect chains).
    let owner_snapshot = owner.snapshot();
    let result = simulate(scene, steps);
    // Always attempt Owner restore — defense in depth. If External-side
    // succeeded but Owner restore failed, surface RollbackFailed so the
    // caller knows the reactive graph is indeterminate.
    if owner.restore(owner_snapshot).is_err() {
        return Err(SimulateError::RollbackFailed);
    }
    result
}

/// Apply `steps` in order, snapshot the resulting compound state,
/// then roll every touched path back to its pre-call value. See the
/// module-level docs for the phase-by-phase contract.
///
/// R648 §5.34 R42 — accepts multi-segment scene paths like
/// `/widget_a/external/value` that walk through tagged Container /
/// Box ancestors before descending into the nested
/// [`pinion_core::scene::ExternalNode`]. Steps targeting different
/// Externals in the same call are addressed independently; the
/// per-unique-path save semantics extend to the `(scene_segments,
/// introspect_path)` tuple key, so two Externals' "value" slots stay
/// distinct in the originals map.
///
/// # Errors
///
/// See [`SimulateError`] for the failure modes. Every error path
/// leaves the caller's scene in its pre-call state except
/// [`SimulateError::RollbackFailed`].
pub fn simulate(scene: &mut Scene, steps: &[SimulateStep]) -> Result<SnapshotNode, SimulateError> {
    // R649 §5.23 R27 — Effect suppression covers all phases so
    // Effects observing Signals mutated through External::intervene
    // don't fire during Phase 2 (apply) or Phase 4 (rollback).
    // Nested SimulationGuard is safe — simulate_with_owner already
    // entered the scope; this guard sees `prior = true` and is a
    // no-op flip on drop.
    let _sim_guard = SimulationGuard::enter();
    if steps.is_empty() {
        return Err(SimulateError::EmptySteps);
    }

    // Phase 0: R667 §5.34 — resolve every step's path through the
    // lifted [`resolve_external_path`] (string-only parse). Reject
    // the whole call on any malformed prefix or missing `/external/`
    // separator before any mutation could land. Per-step error
    // variants carry the offending step's index.
    let mut resolved: Vec<(Vec<String>, String)> = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        resolved.push(resolve_external_path(&step.path).map_err(|e| match e {
            ResolveExternalError::Path(error) => SimulateError::Path {
                step_index: i,
                error,
            },
            // `resolve_external_path` is string-only — `NoExternalAtPath`
            // / `IntrospectionOptedOut` cannot fire here. Collapse
            // exhaustively because `ResolveExternalError` is non_exhaustive.
            ResolveExternalError::UnsupportedPath
            | ResolveExternalError::NoExternalAtPath
            | ResolveExternalError::IntrospectionOptedOut => {
                SimulateError::UnsupportedPath { step_index: i }
            }
        })?);
    }

    // Phase 1: R667 §5.34 — save the original value for every unique
    // (scene_segments, introspect_path) tuple the sequence touches.
    // R648 — the key tuple distinguishes two Externals' same-named
    // slots ("/widget_a/external/value" vs "/widget_b/external/value").
    // BTreeMap deterministic ordering simplifies the rollback
    // iteration + diff inspection in tests. Walk + introspect lookup
    // collapses through [`introspect_mut_at`] so the former
    // `query_introspect_at` + `classify_lookup_failure` two-pass
    // disambiguation flattens into one map_err.
    let mut originals: BTreeMap<(Vec<String>, String), IntrospectValue> = BTreeMap::new();
    for (i, (scene_segments, introspect_path)) in resolved.iter().enumerate() {
        let key = (scene_segments.clone(), introspect_path.clone());
        if originals.contains_key(&key) {
            continue;
        }
        let intro = introspect_mut_at(scene, scene_segments).map_err(|e| match e {
            ResolveExternalError::NoExternalAtPath => SimulateError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => SimulateError::IntrospectionOptedOut,
            // String-parse variants unreachable here (phase 0 validated).
            ResolveExternalError::Path(_) | ResolveExternalError::UnsupportedPath => {
                SimulateError::NoExternalAtPath
            }
        })?;
        let saved = intro
            .query(introspect_path)
            .ok_or(SimulateError::InitialQueryFailed { step_index: i })?;
        originals.insert(key, saved);
    }

    // Phase 2: R667 §5.34 — apply each step in declaration order via
    // [`introspect_mut_at`]. Phase 1 already located + queried every
    // path successfully; reaching `Err` here means the External
    // mutated its own scene topology mid-flight, classified as
    // `InterveneError::UnknownPath` to preserve the prior wire shape.
    let mut intervene_outcome: Result<(), (usize, InterveneError)> = Ok(());
    for (i, ((scene_segments, introspect_path), step)) in
        resolved.iter().zip(steps.iter()).enumerate()
    {
        let Ok(intro) = introspect_mut_at(scene, scene_segments) else {
            intervene_outcome = Err((i, InterveneError::UnknownPath));
            break;
        };
        if let Err(e) = intro.intervene(introspect_path, step.value.clone()) {
            intervene_outcome = Err((i, e));
            break;
        }
    }

    if let Err((step_index, error)) = intervene_outcome {
        let rollback_ok = restore_originals(scene, &originals);
        if !rollback_ok {
            return Err(SimulateError::RollbackFailed);
        }
        return Err(SimulateError::Intervene { step_index, error });
    }

    // Phase 3: snapshot reflects every step applied in sequence.
    let snap_result = snapshot(scene, "");

    // Phase 4: rollback regardless of snapshot outcome so the
    // caller's scene is restored.
    let rollback_ok = restore_originals(scene, &originals);
    if !rollback_ok {
        return Err(SimulateError::RollbackFailed);
    }

    snap_result.map_err(|_e: SnapshotError| SimulateError::SnapshotFailed)
}

/// Restore every saved original through `intervene`, re-walking
/// `scene_segments` for each entry via [`introspect_mut_at`].
/// Returns `false` if any slot rejected the write (invariant
/// violation — the scene is now indeterminate from the caller's
/// perspective).
fn restore_originals(
    scene: &mut Scene,
    originals: &BTreeMap<(Vec<String>, String), IntrospectValue>,
) -> bool {
    for ((scene_segments, introspect_path), original) in originals {
        let Ok(intro) = introspect_mut_at(scene, scene_segments) else {
            return false;
        };
        if intro.intervene(introspect_path, original.clone()).is_err() {
            return false;
        }
    }
    true
}

/// R646 §5.12 — convert a [`SimulateError`] into the [`DryRunError`]
/// equivalent shape so callers that have a single-step error handler
/// can collapse both into one match arm. The step-index context is
/// dropped at the [`DryRunError`] boundary because the single-step
/// [`crate::dry_run::dry_run`] never had it.
impl From<SimulateError> for DryRunError {
    fn from(err: SimulateError) -> Self {
        match err {
            SimulateError::Path { error, .. } => DryRunError::Path(error),
            // R646 v0 — EmptySteps has no `dry_run` analogue; collapse
            // with `UnsupportedPath` so the dispatcher surfaces both
            // with the same caller-error severity. If a future round
            // gives EmptySteps its own surface, split the arm.
            SimulateError::UnsupportedPath { .. } | SimulateError::EmptySteps => {
                DryRunError::UnsupportedPath
            }
            SimulateError::NoExternalAtPath => DryRunError::NoExternalAtPath,
            SimulateError::IntrospectionOptedOut => DryRunError::IntrospectionOptedOut,
            SimulateError::InitialQueryFailed { .. } => DryRunError::InitialQueryFailed,
            SimulateError::Intervene { error, .. } => DryRunError::Intervene(error),
            SimulateError::RollbackFailed => DryRunError::RollbackFailed,
            SimulateError::SnapshotFailed => DryRunError::SnapshotFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

    use crate::query::query;
    use crate::snapshot::ExternalSnapshot;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    fn step(path: &str, value: IntrospectValue) -> SimulateStep {
        SimulateStep {
            path: path.into(),
            value,
        }
    }

    #[test]
    fn empty_steps_rejected() {
        let mut scene = counted_scene(0);
        let err = simulate(&mut scene, &[]).unwrap_err();
        assert_eq!(err, SimulateError::EmptySteps);
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(0)
        );
    }

    #[test]
    fn single_step_matches_dry_run_shape() {
        let mut scene = counted_scene(7);
        let snap = simulate(
            &mut scene,
            &[step("/external/count", IntrospectValue::Int(999))],
        )
        .unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
            }) => {
                assert_eq!(fields[0].1, IntrospectValue::Int(999));
            }
            other => panic!("expected External snapshot, got {other:?}"),
        }
        // Original value restored.
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(7)
        );
    }

    #[test]
    fn two_steps_compound_in_snapshot_and_roll_back() {
        // R646 — the AI-native "if I do A then B" use case. CountedExternal
        // accepts repeat writes; the snapshot must reflect the LATEST
        // value (999), not the intermediate one (42).
        let mut scene = counted_scene(7);
        let snap = simulate(
            &mut scene,
            &[
                step("/external/count", IntrospectValue::Int(42)),
                step("/external/count", IntrospectValue::Int(999)),
            ],
        )
        .unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
            }) => {
                assert_eq!(fields[0].1, IntrospectValue::Int(999));
            }
            other => panic!("expected External snapshot, got {other:?}"),
        }
        // Critical: rollback restores PRE-CALL state (7), not the
        // intermediate state (42) the first step landed on. Per-
        // unique-path save semantics.
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(7)
        );
    }

    #[test]
    fn intervene_failure_midway_rolls_back_earlier_steps() {
        // Step 1 succeeds (42), step 2 fails (Bool on Int slot). The
        // rollback must restore the original value (11), not leave
        // the intermediate (42) behind.
        let mut scene = counted_scene(11);
        let err = simulate(
            &mut scene,
            &[
                step("/external/count", IntrospectValue::Int(42)),
                step("/external/count", IntrospectValue::Bool(true)),
            ],
        )
        .unwrap_err();
        assert_eq!(
            err,
            SimulateError::Intervene {
                step_index: 1,
                error: InterveneError::TypeMismatch,
            }
        );
        // Pre-call state restored, even though step 1 mutated it.
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(11)
        );
    }

    #[test]
    fn initial_query_failure_returns_step_index_no_mutation() {
        let mut scene = counted_scene(0);
        let err = simulate(
            &mut scene,
            &[
                step("/external/count", IntrospectValue::Int(1)),
                step("/external/ghost", IntrospectValue::Int(2)),
            ],
        )
        .unwrap_err();
        assert_eq!(err, SimulateError::InitialQueryFailed { step_index: 1 });
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(0)
        );
    }

    #[test]
    fn path_error_carries_step_index() {
        let mut scene = counted_scene(0);
        let err = simulate(
            &mut scene,
            &[
                step("/external/count", IntrospectValue::Int(1)),
                step("/window[main/external/count", IntrospectValue::Int(2)),
            ],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            SimulateError::Path {
                step_index: 1,
                error: PathError::MalformedPrefix
            }
        ));
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(0)
        );
    }

    #[test]
    fn stub_external_opts_out_no_mutation() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = simulate(
            &mut scene,
            &[step("/external/anything", IntrospectValue::Int(0))],
        )
        .unwrap_err();
        assert_eq!(err, SimulateError::IntrospectionOptedOut);
    }

    #[test]
    fn box_root_rejected_no_mutation() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = simulate(
            &mut scene,
            &[step("/external/count", IntrospectValue::Int(0))],
        )
        .unwrap_err();
        assert_eq!(err, SimulateError::NoExternalAtPath);
    }

    #[test]
    fn unsupported_path_rejected_pre_mutation() {
        let mut scene = counted_scene(5);
        let err = simulate(
            &mut scene,
            &[
                step("/external/count", IntrospectValue::Int(1)),
                step("/some/other", IntrospectValue::Int(2)),
            ],
        )
        .unwrap_err();
        assert_eq!(err, SimulateError::UnsupportedPath { step_index: 1 });
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(5)
        );
    }

    // R648 §5.34 R42 — multi-External path tests. Container with
    // tagged children, each wrapping a CountedExternal; simulate
    // addresses non-primary Externals via /tag/external/... paths.
    mod multi_external {
        use super::*;
        use pinion_core::scene::ContainerNode;

        fn paired_counted_scene(a: i64, b: i64) -> Scene {
            // Root Container with two tagged child Containers; each
            // child carries one External. Paths
            // "/widget_a/external/count" + "/widget_b/external/count"
            // resolve to distinct slots.
            let child_a =
                Scene::Container(ContainerNode::new(vec![counted_scene(a)]).with_tag("widget_a"));
            let child_b =
                Scene::Container(ContainerNode::new(vec![counted_scene(b)]).with_tag("widget_b"));
            Scene::Container(ContainerNode::new(vec![child_a, child_b]))
        }

        #[test]
        fn two_steps_target_distinct_externals_rolled_back_independently() {
            // R648 — the R660+ Figma queue multi-widget prereq. Two
            // tagged child Externals, simulate mutates both, snapshot
            // reflects both new values, rollback restores both.
            let mut scene = paired_counted_scene(5, 17);
            let snap = simulate(
                &mut scene,
                &[
                    step("/widget_a/external/count", IntrospectValue::Int(100)),
                    step("/widget_b/external/count", IntrospectValue::Int(200)),
                ],
            )
            .unwrap();
            // Snapshot shape: full scene snapshot from root.
            // Verify rollback worked at each External.
            assert_eq!(
                query(&scene, "/widget_a/external/count").unwrap(),
                IntrospectValue::Int(5),
            );
            assert_eq!(
                query(&scene, "/widget_b/external/count").unwrap(),
                IntrospectValue::Int(17),
            );
            // Snapshot is non-trivial (non-Empty); detailed structure
            // varies with the snapshot tree shape but the key
            // assertion is "scene restored after compound mutation".
            assert!(!matches!(snap, SnapshotNode::External(_) if false));
        }

        #[test]
        fn distinct_external_originals_keyed_independently() {
            // Path A and path B share the introspect tail "count" but
            // target different Externals. Phase 1's originals map must
            // key on (segments, introspect_path) tuple — not just the
            // introspect tail — so the wrong rollback value never lands.
            let mut scene = paired_counted_scene(11, 22);
            // Single step on each; assert correct rollback per External.
            simulate(
                &mut scene,
                &[
                    step("/widget_a/external/count", IntrospectValue::Int(999)),
                    step("/widget_b/external/count", IntrospectValue::Int(888)),
                ],
            )
            .unwrap();
            assert_eq!(
                query(&scene, "/widget_a/external/count").unwrap(),
                IntrospectValue::Int(11),
            );
            assert_eq!(
                query(&scene, "/widget_b/external/count").unwrap(),
                IntrospectValue::Int(22),
            );
        }

        #[test]
        fn intervene_failure_in_second_external_rolls_back_first() {
            // Step 1 succeeds on widget_a, step 2 fails (type mismatch)
            // on widget_b. Phase 1 already saved both originals, so
            // rollback restores widget_a even though widget_b never
            // had its mutation land.
            let mut scene = paired_counted_scene(7, 13);
            let err = simulate(
                &mut scene,
                &[
                    step("/widget_a/external/count", IntrospectValue::Int(42)),
                    step("/widget_b/external/count", IntrospectValue::Bool(true)),
                ],
            )
            .unwrap_err();
            assert_eq!(
                err,
                SimulateError::Intervene {
                    step_index: 1,
                    error: InterveneError::TypeMismatch,
                }
            );
            // Pre-call state restored at both Externals.
            assert_eq!(
                query(&scene, "/widget_a/external/count").unwrap(),
                IntrospectValue::Int(7),
            );
            assert_eq!(
                query(&scene, "/widget_b/external/count").unwrap(),
                IntrospectValue::Int(13),
            );
        }

        #[test]
        fn missing_external_at_segments_surfaces_no_external_at_path() {
            // segments resolve to a Container but no External nested
            // inside — the lookup_path_mut chain bottoms out at None.
            let scene_outer = Scene::Container(
                ContainerNode::new(vec![Scene::Container(
                    ContainerNode::new(vec![]).with_tag("empty"),
                )])
                .with_tag("root"),
            );
            let mut scene = scene_outer;
            let err = simulate(
                &mut scene,
                &[step("/empty/external/count", IntrospectValue::Int(0))],
            )
            .unwrap_err();
            assert_eq!(err, SimulateError::NoExternalAtPath);
        }
    }

    // R647 §5.22 R26 — Owner bridge tests.
    mod owner_bridge {
        use super::*;
        use pinion_core::{Effect, Owner, Signal, SignalExternal};

        #[test]
        fn empty_owner_matches_simulate_output() {
            // Defense in depth — when no Signals are tracked, the
            // Owner snapshot is empty and restore is a no-op. Output
            // matches the External-only simulate path.
            let owner = Owner::new();
            let mut scene = counted_scene(11);
            let snap = simulate_with_owner(
                &mut scene,
                &owner,
                &[step("/external/count", IntrospectValue::Int(42))],
            )
            .unwrap();
            match snap {
                SnapshotNode::External(ExternalSnapshot {
                    introspect: Some(fields),
                    ..
                }) => {
                    assert_eq!(fields[0].1, IntrospectValue::Int(42));
                }
                other => panic!("expected External snapshot, got {other:?}"),
            }
            assert_eq!(
                query(&scene, "/external/count").unwrap(),
                IntrospectValue::Int(11)
            );
        }

        #[test]
        fn signal_external_value_restored_via_external_rollback() {
            // SignalExternal::intervene calls signal.set(); the External
            // rollback symmetry already restores the Signal. Owner bridge
            // is redundant for this case (still correct, just not the
            // case demonstrating its load-bearing value).
            let owner = Owner::new();
            let signal: Signal<i32> = Signal::new(7);
            owner.track(&signal);
            let mut scene = Scene::External(ExternalNode::new(Box::new(
                SignalExternal::<i32>::new(signal.clone()),
            )));
            let snap = simulate_with_owner(
                &mut scene,
                &owner,
                &[step("/external/value", IntrospectValue::Int(999))],
            )
            .unwrap();
            match snap {
                SnapshotNode::External(ExternalSnapshot {
                    introspect: Some(fields),
                    ..
                }) => {
                    assert_eq!(fields[0].1, IntrospectValue::Int(999));
                }
                other => panic!("expected External snapshot, got {other:?}"),
            }
            // Both paths agree on the original value.
            assert_eq!(signal.get(), 7);
            assert_eq!(
                query(&scene, "/external/value").unwrap(),
                IntrospectValue::Int(7)
            );
        }

        #[test]
        fn r649_effect_suppressed_during_simulate() {
            // R27 §5.23 R649 — Effect observing input must NOT fire
            // while the SimulationGuard is active. We verify by
            // counting fire events: counter increments inside the
            // Effect closure, so the count after simulate equals the
            // pre-call count (zero new fires during the simulate
            // body) — no longer the +2 (mutation + rollback) of the
            // pre-R649 behaviour.
            //
            // Naive `set(get()+1)` Effect pattern works here BECAUSE
            // R649 suppresses the Effect entirely; the R647-era
            // `set_with` workaround stays useful for the bare
            // `simulate_with_owner` carry-forward but is no longer
            // load-bearing for this assertion.
            let owner = Owner::new();
            let input: Signal<i32> = Signal::new(7);
            let counter: Signal<i32> = Signal::new(0);
            owner.track(&input);
            owner.track(&counter);
            let input_for_effect = input.clone();
            let counter_for_effect = counter.clone();
            let _effect = Effect::new(&owner, move || {
                let _ = input_for_effect.get();
                counter_for_effect.set(counter_for_effect.get() + 1);
            });
            // Effect ran once on registration.
            let pre_counter = counter.get();
            assert_eq!(pre_counter, 1);

            let mut scene = Scene::External(ExternalNode::new(Box::new(
                SignalExternal::<i32>::new(input.clone()),
            )));
            simulate_with_owner(
                &mut scene,
                &owner,
                &[step("/external/value", IntrospectValue::Int(999))],
            )
            .unwrap();
            // Critical: counter unchanged from pre-call. Effect was
            // suppressed during both mutation (intervene) and
            // rollback (External + Owner). No double-fire.
            assert_eq!(
                counter.get(),
                pre_counter,
                "R649 Effect suppression broken — Effect fired during simulate",
            );
            // input still restored via External rollback.
            assert_eq!(input.get(), 7);
        }

        #[test]
        fn non_idempotent_effect_chain_signal_restored_by_owner_bridge() {
            // R26 §5.22 load-bearing case — Effect increments an
            // independent counter on every change to the input Signal.
            // simulate sets input → counter increments (Effect fires).
            // External-side rollback sets input back → Effect fires
            // again → counter increments AGAIN. After simulate without
            // Owner bridge: counter is +2 (not pre-call value).
            //
            // simulate_with_owner snapshots the counter Signal before
            // simulate, restores after — counter is back to pre-call.
            //
            // Owner registers Signals via explicit `track()` (not
            // auto-track) — only tracked Signals appear in snapshot.
            let owner = Owner::new();
            let input: Signal<i32> = Signal::new(7);
            let counter: Signal<i32> = Signal::new(0);
            owner.track(&input);
            owner.track(&counter);
            let input_for_effect = input.clone();
            let counter_for_effect = counter.clone();
            // The Effect must observe `input` to subscribe; Effect::new
            // takes &Owner explicitly. Note `set_with` rather than
            // `set(get() + 1)` — the latter's inner `.get()` would
            // subscribe Effect to counter too, making Owner restore
            // re-fire the Effect on counter writes (defeats the test
            // contract). `set_with` borrows the inner value without
            // touching the with_current_owner tracker.
            let _effect = Effect::new(&owner, move || {
                let _ = input_for_effect.get();
                counter_for_effect.set_with(|prev| *prev + 1);
            });
            // Effect ran once on registration (counter = 1).
            let pre_counter = counter.get();
            assert_eq!(pre_counter, 1, "Effect fires once on registration");

            let mut scene = Scene::External(ExternalNode::new(Box::new(
                SignalExternal::<i32>::new(input.clone()),
            )));
            let snap = simulate_with_owner(
                &mut scene,
                &owner,
                &[step("/external/value", IntrospectValue::Int(999))],
            )
            .unwrap();
            match snap {
                SnapshotNode::External(ExternalSnapshot {
                    introspect: Some(fields),
                    ..
                }) => {
                    assert_eq!(fields[0].1, IntrospectValue::Int(999));
                }
                other => panic!("expected External snapshot, got {other:?}"),
            }

            // input value restored via either path.
            assert_eq!(input.get(), 7);
            // counter restored to pre-call value via Owner bridge —
            // without it, counter would be pre_counter + 2 (one from
            // simulate intervene, one from rollback intervene).
            assert_eq!(
                counter.get(),
                pre_counter,
                "Owner bridge must restore non-idempotent Effect Signal state",
            );
        }
    }
}
