//! `scene/animate_settle` + `scene/animate_cancel` RPC method
//! dispatch — R629 §5.28 + §5.7.
//!
//! Exposes the R623 [`Animation<T>`] control surface over the
//! JSON-RPC wire so AI agents can externally drive animation
//! settlement without polling [`scene/animation_state`] for the
//! `active: false` flip.
//!
//! [`Animation<T>`]: pinion_core::animation::Animation
//!
//! ## Bulk-walk shape (no per-tag dispatch)
//!
//! Both methods walk every animation registered on `runtime_owner`
//! and its descendant scopes (mirroring
//! [`Owner::tick_animations`](pinion_core::reactive::Owner::tick_animations)
//! / [`Owner::any_animation_active`](pinion_core::reactive::Owner::any_animation_active)).
//! Per-tag addressing — the R608-R612 setter axis pattern
//! (`scene/set_text`, `scene/set_scroll_offset`, …) — is **not**
//! offered because animations are application-private substate
//! (a [`ThemeProvider`](pinion_core::theme::ThemeProvider) owns
//! its own [`Animation<AnimVec4>`](pinion_core::animation::Animation)
//! internally; widgets bind their own fades; there is no canonical
//! framework-level animation-tag → handle registry).
//!
//! `scene/animate_reset` (the third R623 method, `Animation::reset(value: T)`)
//! is **deferred** for the same reason — `reset` is typed on the
//! caller-supplied `T`; without a per-tag animation registry there
//! is no wire-form that can faithfully carry the generic `T` over
//! JSON. The deferral is documented at
//! [[abstraction-needs-second-consumer]] — landing
//! `scene/animate_reset` requires a real second consumer that
//! provides the typed-dispatch substrate.
//!
//! ## Wire shape
//!
//! Request (`scene/animate_settle`):
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/animate_settle",
//!   "id": 1
//! }
//! ```
//!
//! `params` is omitted (or `{}` — both forms accepted). Response:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": { "visited": 2 }
//! }
//! ```
//!
//! `visited` is the count of animation registrations the walk
//! touched (including ones that were already at rest). A `0` reply
//! is the canonical "nothing to do" signal — agents can use it to
//! verify the runtime owner has no live animations without taking
//! a second [`scene/animation_state`] snapshot.
//!
//! Request (`scene/animate_cancel`): identical shape; the method
//! name selects the semantic (freeze-at-current vs. jump-to-target).
//!
//! ## Side-effect contract
//!
//! Both methods MUTATE — they write the underlying
//! [`Signal<T>`](pinion_core::reactive::Signal) on every visited
//! animation. The dispatcher's
//! [`HandlerKind::Mutate`](mod@crate::dispatch) tag bumps
//! [`SceneRevision`](pinion_core::SceneRevision) so subsequent
//! `If-Match`-style OCC checks pick the change up.

use pinion_core::reactive::Owner;
use serde::Serialize;

/// Typed errors the [`animate_settle`] / [`animate_cancel`]
/// dispatchers can return. Every variant maps onto a JSON-RPC
/// `-32602 Invalid params` response at the dispatch layer with the
/// variant name in `error.data` so AI agents pattern-match without
/// parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum AnimateControlError {
    /// The embedder did not register a runtime [`Owner`] on the
    /// dispatch context. Without the substrate's root owner the
    /// bulk walk has no starting scope.
    RuntimeOwnerUnavailable,
}

/// Bulk-control outcome — the count of animation registrations the
/// walk visited (including already-at-rest entries). Same shape for
/// both `settle` and `cancel`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AnimateControlOutcome {
    /// Number of [`Tickable`](pinion_core::animation::Tickable)
    /// registrations the walk touched. `0` when the runtime owner
    /// holds no animations.
    pub visited: usize,
}

/// R629 §5.28 — bulk-settle every animation on `runtime_owner` and
/// its descendant scopes. Each visited animation jumps to its
/// internal target with zero velocity; after the call
/// [`Owner::any_animation_active`] returns `false` (modulo a
/// non-NaN epsilon).
///
/// # Errors
///
/// - [`AnimateControlError::RuntimeOwnerUnavailable`] — see field
///   docs.
///
/// # Side effects
///
/// Mutates the underlying [`Signal`](pinion_core::reactive::Signal)
/// on every visited animation. The dispatch layer's
/// [`HandlerKind::Mutate`](mod@crate::dispatch) tag bumps
/// [`SceneRevision`](pinion_core::SceneRevision).
pub fn animate_settle(
    runtime_owner: Option<&Owner>,
) -> Result<AnimateControlOutcome, AnimateControlError> {
    let Some(owner) = runtime_owner else {
        return Err(AnimateControlError::RuntimeOwnerUnavailable);
    };
    Ok(AnimateControlOutcome {
        visited: owner.settle_animations(),
    })
}

/// R629 §5.28 — bulk-cancel every animation on `runtime_owner` and
/// its descendant scopes. Each visited animation freezes at its
/// current value with zero velocity; after the call
/// [`Owner::any_animation_active`] returns `false`.
///
/// # Errors
///
/// - [`AnimateControlError::RuntimeOwnerUnavailable`] — see field
///   docs.
///
/// # Side effects
///
/// Mutates the underlying [`Signal`](pinion_core::reactive::Signal)
/// on every visited animation. The dispatch layer's
/// [`HandlerKind::Mutate`](mod@crate::dispatch) tag bumps
/// [`SceneRevision`](pinion_core::SceneRevision).
pub fn animate_cancel(
    runtime_owner: Option<&Owner>,
) -> Result<AnimateControlOutcome, AnimateControlError> {
    let Some(owner) = runtime_owner else {
        return Err(AnimateControlError::RuntimeOwnerUnavailable);
    };
    Ok(AnimateControlOutcome {
        visited: owner.cancel_animations(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::animation::{Animation, SpringConfig};

    #[test]
    fn r629_animate_settle_missing_runtime_owner_errors() {
        let err = animate_settle(None).unwrap_err();
        assert_eq!(err, AnimateControlError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r629_animate_cancel_missing_runtime_owner_errors() {
        let err = animate_cancel(None).unwrap_err();
        assert_eq!(err, AnimateControlError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r629_animate_settle_empty_owner_returns_zero_visited() {
        let owner = Owner::new();
        let out = animate_settle(Some(&owner)).unwrap();
        assert_eq!(out.visited, 0);
    }

    #[test]
    fn r629_animate_cancel_empty_owner_returns_zero_visited() {
        let owner = Owner::new();
        let out = animate_cancel(Some(&owner)).unwrap();
        assert_eq!(out.visited, 0);
    }

    #[test]
    fn r629_animate_settle_lands_animation_at_target() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(42.0);
        owner.tick_animations(0.016);
        assert!(a.value() < 42.0, "mid-flight precondition");
        let out = animate_settle(Some(&owner)).unwrap();
        assert_eq!(out.visited, 1);
        assert!((a.value() - 42.0).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }

    #[test]
    fn r629_animate_cancel_freezes_animation_at_current() {
        let owner = Owner::new();
        let a = Animation::new(&owner, 0.0_f32, SpringConfig::DEFAULT);
        a.set_target(100.0);
        for _ in 0..5 {
            owner.tick_animations(0.016);
        }
        let mid = a.value();
        assert!(mid > 0.0 && mid < 100.0, "mid-flight precondition");
        let out = animate_cancel(Some(&owner)).unwrap();
        assert_eq!(out.visited, 1);
        assert!((a.value() - mid).abs() < f32::EPSILON);
        assert!(a.is_at_rest());
    }
}
