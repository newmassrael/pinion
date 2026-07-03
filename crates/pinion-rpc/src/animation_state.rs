//! `scene/animation_state` RPC method dispatch — R600 §5.28 + §5.7.
//!
//! Exposes [`Owner::any_animation_active`](pinion_core::reactive::Owner::any_animation_active)
//! over the JSON-RPC surface so AI agents can observe animation
//! settlement without polling [`scene/snapshot`] for visual stability.
//!
//! ## Why a dedicated method
//!
//! [`scene/waitFor`](fn@crate::wait_for) polls
//! [`scene/query`](fn@crate::query) for an [`IntrospectValue`](pinion_core::external::IntrospectValue) match;
//! animations are a *spring-solver convergence* concept that does
//! not surface through [`External::introspect`](pinion_core::external::External::introspect). Agents that want
//! "wait until the theme fade settles" or "snapshot only after the
//! scroll animation rests" currently have to take two snapshots and
//! diff — wasteful on the wire and brittle (anti-aliasing jitter
//! breaks pixel equality).
//!
//! [`Owner::any_animation_active`] already exists since R51.147 §5.28
//! as the substrate's "should the vsync loop request another frame?"
//! predicate. R600 surfaces the same primitive over RPC, mirroring
//! the textbook "if the framework knows it, so should the agent"
//! introspection contract (§2#2 + §2#7).
//!
//! ## Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "active": false,
//!     "epsilon": 0.01
//!   }
//! }
//! ```
//!
//! Request:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/animation_state",
//!   "params": { "epsilon": 0.001 },
//!   "id": 1
//! }
//! ```
//!
//! `params.epsilon` is optional; when omitted the dispatcher uses
//! the substrate-level
//! [`DEFAULT_REST_EPSILON`]
//! (`0.01`). The chosen value is echoed back in the response so the
//! agent can record exactly which settlement threshold the framework
//! evaluated against.
//!
//! ## Side-effect contract
//!
//! Read-only. The call walks
//! [`Owner::any_animation_active`] (which itself only borrows the
//! registry to snapshot via `Rc::clone`); no signal is read inside
//! an active reactive computation, so the introspection neither
//! subscribes the framework nor schedules a re-paint.

use pinion_core::animation::DEFAULT_REST_EPSILON;
use pinion_core::reactive::Owner;
use serde::Serialize;

/// Typed errors the [`animation_state`] dispatcher can return. Every
/// variant maps onto a JSON-RPC `-32602 Invalid params` response at
/// the dispatch layer with the variant name in `error.data` so AI
/// agents can pattern-match without parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationStateError {
    /// The embedder did not register a
    /// [`runtime_owner`](crate::DispatchContext) on the dispatch
    /// context. Without the substrate's root [`Owner`] the
    /// settlement walk has no starting scope.
    RuntimeOwnerUnavailable,
    /// `params.epsilon` was supplied but failed validation —
    /// negative, NaN, or +∞ (the only finite floats
    /// [`Tickable::is_at_rest`](pinion_core::animation::Tickable::is_at_rest) is documented to accept are
    /// `>= 0.0`). Carries the rejected value so the agent can adjust.
    InvalidEpsilon { value: f64 },
}

/// Settlement snapshot returned by [`animation_state`]. `active`
/// mirrors the spring solver's "is anything still moving?" decision;
/// `epsilon` echoes the threshold the framework evaluated against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AnimationStateOutcome {
    /// `true` when at least one registered animation reports
    /// [`Tickable::is_at_rest(epsilon) = false`](pinion_core::animation::Tickable::is_at_rest).
    /// `false` once every animation has settled (or no animations
    /// are registered at all).
    pub active: bool,
    /// Threshold the framework used. Equals the request's
    /// `params.epsilon` when supplied, or
    /// [`DEFAULT_REST_EPSILON`] when omitted. Echoed so
    /// the agent records the *evaluated* threshold, not just the
    /// *requested* one.
    pub epsilon: f32,
}

/// Snapshot the animation settlement state on `runtime_owner` and
/// its descendant scopes.
///
/// Pass `epsilon = None` to use [`DEFAULT_REST_EPSILON`]
/// (`0.01`). Supplied values must be a finite non-negative `f32`;
/// `NaN` / negative / non-finite values surface as
/// [`AnimationStateError::InvalidEpsilon`].
///
/// # Errors
///
/// - [`AnimationStateError::RuntimeOwnerUnavailable`] — see field
///   docs.
/// - [`AnimationStateError::InvalidEpsilon`] — see field docs.
///
/// # Side effects
///
/// None. The walk borrows the registry only to snapshot via
/// `Rc::clone` (mirroring [`Owner::tick_animations`]'s borrow
/// discipline); no reactive computation is established.
pub fn animation_state(
    runtime_owner: Option<&Owner>,
    epsilon: Option<f32>,
) -> Result<AnimationStateOutcome, AnimationStateError> {
    let Some(owner) = runtime_owner else {
        return Err(AnimationStateError::RuntimeOwnerUnavailable);
    };
    let resolved = match epsilon {
        Some(value) if !value.is_finite() || value < 0.0 => {
            return Err(AnimationStateError::InvalidEpsilon {
                value: f64::from(value),
            });
        }
        Some(value) => value,
        None => DEFAULT_REST_EPSILON,
    };
    Ok(AnimationStateOutcome {
        active: owner.any_animation_active(resolved),
        epsilon: resolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::animation::{Animation, SpringConfig};
    use pinion_core::reactive::Owner;

    /// Critically-damped spring config matching the canonical
    /// [`THEME_FADE_SPRING`](pinion_core::theme::THEME_FADE_SPRING)
    /// shape (`omega_n=20`, `zeta=1`); short enough settle time the
    /// test does not need to advance frames to assert at-rest
    /// behavior.
    fn fast_spring() -> SpringConfig {
        SpringConfig::new(400.0, 40.0, 1.0)
    }

    #[test]
    fn r600_missing_runtime_owner_errors() {
        let err = animation_state(None, None).unwrap_err();
        assert_eq!(err, AnimationStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r600_negative_epsilon_rejected() {
        let owner = Owner::new();
        let err = animation_state(Some(&owner), Some(-0.001)).unwrap_err();
        assert!(matches!(err, AnimationStateError::InvalidEpsilon { .. }));
    }

    #[test]
    fn r600_nan_epsilon_rejected() {
        let owner = Owner::new();
        let err = animation_state(Some(&owner), Some(f32::NAN)).unwrap_err();
        assert!(matches!(err, AnimationStateError::InvalidEpsilon { .. }));
    }

    #[test]
    fn r600_infinite_epsilon_rejected() {
        let owner = Owner::new();
        let err = animation_state(Some(&owner), Some(f32::INFINITY)).unwrap_err();
        assert!(matches!(err, AnimationStateError::InvalidEpsilon { .. }));
    }

    #[test]
    fn r600_empty_owner_is_inactive() {
        let owner = Owner::new();
        let outcome = animation_state(Some(&owner), None).unwrap();
        assert!(!outcome.active);
        assert!((outcome.epsilon - DEFAULT_REST_EPSILON).abs() < 1e-9);
    }

    #[test]
    fn r600_active_spring_reports_active_true() {
        // Register a spring animation that has not yet settled —
        // initial value 0.0, target 1.0; without ticking the spring,
        // the displacement equals the target diff, well above the
        // 0.01 rest epsilon.
        let owner = Owner::new();
        let anim = Animation::new(&owner, 0.0_f32, fast_spring());
        // Reach for the animation's target so the spring is not
        // already at rest. The default target equals the initial
        // value, so explicitly retarget to perturb the system.
        anim.set_target(1.0);
        let outcome = animation_state(Some(&owner), None).unwrap();
        assert!(
            outcome.active,
            "spring with non-zero target diff must be active"
        );
    }

    #[test]
    fn r600_epsilon_echoed_back_unchanged() {
        let owner = Owner::new();
        let outcome = animation_state(Some(&owner), Some(0.001)).unwrap();
        assert!((outcome.epsilon - 0.001).abs() < 1e-9);
    }

    #[test]
    fn r600_default_epsilon_matches_animation_default() {
        let owner = Owner::new();
        let outcome = animation_state(Some(&owner), None).unwrap();
        assert!(
            (outcome.epsilon - DEFAULT_REST_EPSILON).abs() < f32::EPSILON,
            "default epsilon must equal DEFAULT_REST_EPSILON",
        );
    }

    #[test]
    fn r600_child_scope_active_propagates_to_parent_walk() {
        // R51.147 invariant: any_animation_active is recursive. A
        // child scope's active animation must report as active when
        // queried at the parent.
        let parent = Owner::new();
        let child = Owner::new_child(&parent);
        let anim = Animation::new(&child, 0.0_f32, fast_spring());
        anim.set_target(1.0);
        let parent_outcome = animation_state(Some(&parent), None).unwrap();
        assert!(parent_outcome.active, "child-scope animation propagates");
    }

    #[test]
    fn r600_outcome_serializes_to_expected_shape() {
        let owner = Owner::new();
        let outcome = animation_state(Some(&owner), Some(0.05)).unwrap();
        let json = serde_json::to_value(outcome).unwrap();
        assert_eq!(json["active"], false);
        // f32 → f64 serialization can introduce small drift; assert
        // within tolerance rather than exact equality.
        let echoed = json["epsilon"].as_f64().expect("epsilon is a number");
        assert!((echoed - 0.05).abs() < 1e-5);
        let obj = json.as_object().expect("outcome is a JSON object");
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let key_strs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        assert_eq!(key_strs, vec!["active", "epsilon"]);
    }
}
