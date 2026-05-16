//! Concrete [`Proposal`] variants the JSON-RPC boundary materialises
//! from wire payloads (§5.34 R40.5).
//!
//! [`TypedProposal`] is the closed enum implementing the open
//! [`Proposal`] trait. Variants are added one at a time per sub-slice;
//! R40.5 lands `SetSignal` as the first since it is the smallest
//! (scalar reactive write). `ReplaceView`, `SetStyle`, and
//! `DispatchIntent` arrive in subsequent R40.x sub-slices.
//!
//! The enum is `#[non_exhaustive]` so adding variants is non-breaking
//! per Hyrum / Bloch API-evolution conventions.

use pinion_core::Scene;

use crate::rewind::{rewind, RewindError};

use super::Proposal;

/// Closed enum of typed proposals carried by the preview ledger
/// (§5.34 R40.5). Each variant carries its own payload shape; the
/// shared [`Proposal`] surface (`target_path` / `affected_paths`) is
/// derived from the variant.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum TypedProposal {
    /// Set a reactive [`pinion_core::Signal`] to a new value while a
    /// preview is in flight. `target_path` is the scene anchor the
    /// AI agent is reasoning about (typically the widget whose render
    /// output reflects the signal); `signal_path` is the addressable
    /// signal id within the scene (e.g. `/button[main]/count`); the
    /// JSON value is the proposed new state. Type coercion (i64 / f64
    /// / String / JSON) happens at apply time (R40.6) so the ledger
    /// can store an opaque payload without committing to the
    /// signal's `T` here.
    SetSignal {
        /// Scene path the AI agent anchors on (overlay highlight,
        /// `scene/bbox` lookup target).
        target_path: String,
        /// Addressable signal id. Resolved against the live scene at
        /// apply time; mismatch surfaces as
        /// `crate::preview::ApplyError::BaseRevisionConflict` only if
        /// the `scene_revision` moved — a missing signal is a separate
        /// apply-time error (R40.6).
        signal_path: String,
        /// New value. Stored verbatim as `serde_json::Value`; type
        /// coercion is the apply step's responsibility.
        value: serde_json::Value,
    },
}

impl Proposal for TypedProposal {
    fn target_path(&self) -> &str {
        match self {
            Self::SetSignal { target_path, .. } => target_path,
        }
    }

    fn affected_paths(&self) -> Vec<String> {
        match self {
            Self::SetSignal { target_path, .. } => vec![target_path.clone()],
        }
    }

    fn apply(&self, scene: &mut Scene) -> Result<(), String> {
        match self {
            Self::SetSignal {
                signal_path, value, ..
            } => apply_set_signal(scene, signal_path, value),
        }
    }
}

fn apply_set_signal(
    scene: &mut Scene,
    signal_path: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let intro_value = json_to_introspect_value(value)
        .ok_or_else(|| "UnsupportedValueShape".to_string())?;
    rewind(scene, signal_path, intro_value).map_err(rewind_error_tag)
}

fn rewind_error_tag(err: RewindError) -> String {
    match err {
        RewindError::Path(_) => "Path".to_string(),
        RewindError::UnsupportedPath => "UnsupportedPath".to_string(),
        RewindError::NoExternalAtPath => "NoExternalAtPath".to_string(),
        RewindError::IntrospectionOptedOut => "IntrospectionOptedOut".to_string(),
        RewindError::Intervene(_) => "Intervene".to_string(),
    }
}

fn json_to_introspect_value(
    v: &serde_json::Value,
) -> Option<pinion_core::external::IntrospectValue> {
    use pinion_core::external::IntrospectValue;
    match v {
        serde_json::Value::Null => Some(IntrospectValue::Null),
        serde_json::Value::Bool(b) => Some(IntrospectValue::Bool(*b)),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(IntrospectValue::Int)
            .or_else(|| n.as_f64().map(IntrospectValue::Float)),
        serde_json::Value::String(s) => Some(IntrospectValue::Text(s.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Some(IntrospectValue::Json(v.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_signal_target_path_is_borrowed() {
        let p = TypedProposal::SetSignal {
            target_path: "/widget[counter]".to_string(),
            signal_path: "/widget[counter]/count".to_string(),
            value: serde_json::json!(42),
        };
        assert_eq!(p.target_path(), "/widget[counter]");
    }

    #[test]
    fn set_signal_affected_paths_contains_target() {
        let p = TypedProposal::SetSignal {
            target_path: "/anchor".to_string(),
            signal_path: "/some/signal".to_string(),
            value: serde_json::json!(true),
        };
        let affected = p.affected_paths();
        assert_eq!(affected, vec!["/anchor".to_string()]);
    }

    #[test]
    fn set_signal_clones_cheaply_via_derive() {
        let p = TypedProposal::SetSignal {
            target_path: "/a".to_string(),
            signal_path: "/b".to_string(),
            value: serde_json::json!("hello"),
        };
        let q = p.clone();
        // Ensure both still report the same paths after Clone.
        assert_eq!(p.target_path(), q.target_path());
        assert_eq!(p.affected_paths(), q.affected_paths());
    }
}
