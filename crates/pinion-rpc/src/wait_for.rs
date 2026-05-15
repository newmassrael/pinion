//! `scene/waitFor` RPC method dispatch (§5.12 method 6 of 7, R16 slice 15).
//!
//! v0 is **sync polling**: the loop calls [`crate::query`] up to
//! `max_attempts` times and returns the first match. Because the loop
//! does not yield between iterations and does not inject events, the
//! polled value is deterministic across iterations — `matched=true` on
//! iteration 1 or `matched=false` after `max_attempts`, with no useful
//! middle ground. The shape exists so AI clients can author wait-for
//! intent today; the *actual* iteration becomes meaningful once the
//! async boundary lands (§6.3) and external state arrival can land
//! between polls.

use pinion_core::external::IntrospectValue;
use pinion_core::Scene;

use crate::path::PathError;
use crate::query::{query, QueryError};

/// Outcome of [`wait_for`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct WaitOutcome {
    /// `true` when one of the polls matched `target`.
    pub matched: bool,
    /// 1-indexed iteration on which the match landed, or `max_attempts`
    /// when no match was found.
    pub attempts: u32,
    /// Value observed on the final poll (whether or not it matched).
    pub final_value: IntrospectValue,
}

/// Reasons [`wait_for`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitForError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Underlying `scene/query` failed.
    Query(QueryError),
    /// `max_attempts = 0` is rejected — the caller likely meant 1.
    ZeroAttempts,
}

impl From<PathError> for WaitForError {
    fn from(err: PathError) -> Self {
        WaitForError::Path(err)
    }
}

impl From<QueryError> for WaitForError {
    fn from(err: QueryError) -> Self {
        WaitForError::Query(err)
    }
}

/// Poll `query(scene, raw_path)` up to `max_attempts` times, returning
/// the first match against `target`.
///
/// # Errors
///
/// Returns [`WaitForError`] when the path fails to resolve, the
/// underlying query rejects the path, or `max_attempts` is zero.
pub fn wait_for(
    scene: &Scene,
    raw_path: &str,
    target: &IntrospectValue,
    max_attempts: u32,
) -> Result<WaitOutcome, WaitForError> {
    if max_attempts == 0 {
        return Err(WaitForError::ZeroAttempts);
    }

    let mut last_value = IntrospectValue::Null;
    for attempt in 1..=max_attempts {
        let value = query(scene, raw_path)?;
        if value == *target {
            return Ok(WaitOutcome {
                matched: true,
                attempts: attempt,
                final_value: value,
            });
        }
        last_value = value;
    }

    Ok(WaitOutcome {
        matched: false,
        attempts: max_attempts,
        final_value: last_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::ExternalNode;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn matches_on_first_attempt_when_target_equals_current() {
        let scene = counted_scene(42);
        let outcome = wait_for(&scene, "/external/count", &IntrospectValue::Int(42), 5).unwrap();
        assert!(outcome.matched);
        assert_eq!(outcome.attempts, 1);
        assert_eq!(outcome.final_value, IntrospectValue::Int(42));
    }

    #[test]
    fn never_matches_returns_max_attempts() {
        let scene = counted_scene(0);
        let outcome = wait_for(&scene, "/external/count", &IntrospectValue::Int(999), 7).unwrap();
        assert!(!outcome.matched);
        assert_eq!(outcome.attempts, 7);
        assert_eq!(outcome.final_value, IntrospectValue::Int(0));
    }

    #[test]
    fn zero_attempts_is_rejected() {
        let scene = counted_scene(0);
        let err = wait_for(&scene, "/external/count", &IntrospectValue::Int(0), 0).unwrap_err();
        assert_eq!(err, WaitForError::ZeroAttempts);
    }

    #[test]
    fn query_error_propagates() {
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err =
            wait_for(&scene, "/external/anything", &IntrospectValue::Null, 3).unwrap_err();
        assert_eq!(err, WaitForError::Query(QueryError::IntrospectionOptedOut));
    }

    #[test]
    fn window_prefix_short_circuits() {
        let scene = counted_scene(11);
        let outcome = wait_for(
            &scene,
            "/window[main]/external/count",
            &IntrospectValue::Int(11),
            3,
        )
        .unwrap();
        assert!(outcome.matched);
    }
}
