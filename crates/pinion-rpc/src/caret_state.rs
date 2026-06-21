//! `scene/caret_state` RPC method dispatch — R604 §5.22 + §5.7.
//!
//! Fifth and final reactive-substrate introspection method, closing
//! the AI-first observability matrix opened by [`crate::theme`]
//! (R598/R599), [`crate::animation_state`] (R600),
//! [`crate::scroll_state`] (R602), [`crate::text_state`] (R603).
//!
//! Projects [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink)
//! — the blink phase + the master gate + the canonical phase
//! period — so AI agents can answer "is the caret on this frame?"
//! and "is the caret driver running at all?" without snapshotting
//! pixels.
//!
//! ## Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "tag": "username",
//!     "visible": true,
//!     "enabled": true,
//!     "period_secs": 0.530
//!   }
//! }
//! ```
//!
//! Request:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/caret_state",
//!   "params": { "tag": "username" },
//!   "id": 1
//! }
//! ```
//!
//! `params.tag` is **required** — caret blinks are per-field tagged
//! (matching the [`TextEditState`] tag by convention so the two
//! `Owner::cache` slots share a symbolic identifier).
//!
//! ## Why both `visible` and `enabled`
//!
//! `visible` is the immediate this-frame paint decision (the AI-first
//! "is the caret on screen right now?"). `enabled` is the gate
//! controlling whether the blink driver advances at all — a
//! disabled caret is always hidden, but an enabled caret alternates
//! every [`CaretBlink::PERIOD_SECS`]. Agents that need to wait for
//! the next phase flip read both: `enabled = true && visible =
//! false` means "wait `<=PERIOD_SECS` and the caret will appear",
//! `enabled = false` means "the caret driver is paused; no flip
//! coming until [`CaretBlink::set_enabled`] fires".
//!
//! ## `period_secs` constant
//!
//! Echoed back at the RPC surface so agents do not have to hard-code
//! the cross-platform 530 ms blink period that pinion mirrors
//! (Chromium / Firefox / Safari / Windows-default). Sourced verbatim
//! from [`CaretBlink::PERIOD_SECS`] so a future tuning round
//! propagates through the wire automatically.

use pinion_core::reactive::Owner;
use pinion_core::widgets::caret_blink::CaretBlink;
use serde::Serialize;

use crate::substrate_introspect::{SubstrateIntrospectError, lookup};

/// Typed errors the [`caret_state`] dispatcher can return. R607
/// §5.7 §5.22 aliased to [`SubstrateIntrospectError`]. See the
/// `scroll_state` companion for the lift rationale.
pub type CaretStateError = SubstrateIntrospectError;

/// Snapshot of the bound [`CaretBlink`]'s observable surface plus
/// the canonical blink period for client-side phase-flip ETA math.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaretStateOutcome {
    /// Echoes the request's `params.tag`. Owned `String` (post-R605
    /// `Box::leak` elimination).
    pub tag: String,
    /// Whether the caret is currently in the "drawn" phase. Flips
    /// every [`Self::period_secs`] when [`Self::enabled`] is `true`.
    pub visible: bool,
    /// Master gate — when `false`, the caret is hidden and the
    /// blink driver does not advance. `true` for a focused text
    /// field; `false` for an unfocused / disabled field.
    pub enabled: bool,
    /// Canonical half-period of the blink in seconds. Sourced from
    /// [`CaretBlink::PERIOD_SECS`] (currently `0.530`, matching the
    /// Chromium / Firefox / Safari / Windows default UX).
    pub period_secs: f32,
}

impl CaretStateOutcome {
    fn from_state(tag: &str, state: &CaretBlink) -> Self {
        Self {
            tag: tag.to_owned(),
            visible: state.visible(),
            enabled: state.enabled(),
            period_secs: CaretBlink::PERIOD_SECS,
        }
    }
}

/// Snapshot the [`CaretBlink`] cached at `tag` on `runtime_owner`.
///
/// # Errors
///
/// - [`CaretStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached.
/// - [`CaretStateError::NotBound`] — owner has no caret-blink
///   state cached under `tag`.
///
/// # Side effects
///
/// None. The [`Owner::cache_contains`] gate routes the no-slot case
/// to [`CaretStateError::NotBound`] so the call never materializes
/// a phantom blink driver — important because
/// [`use_caret_blink`] registers the driver with the owner's
/// animation tick registry, so a lazy-init by the introspection
/// path would silently grow the workload.
pub fn caret_state(
    runtime_owner: Option<&Owner>,
    tag: &str,
) -> Result<CaretStateOutcome, CaretStateError> {
    lookup::<CaretBlink, _, _>(runtime_owner, tag, CaretStateOutcome::from_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    // R622 §5.7 — 1-line typed wrapper specializing
    // crate::test_fixtures::bind_state to CaretBlink.
    fn bind_state(owner: &Owner, tag: &'static str) -> std::rc::Rc<CaretBlink> {
        crate::test_fixtures::bind_state::<CaretBlink>(owner, tag)
    }

    // ─────────────────────────────────────────────────────────────────
    // Failure modes
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r604_missing_runtime_owner_errors() {
        let err = caret_state(None, "field").unwrap_err();
        assert_eq!(err, CaretStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r604_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = caret_state(Some(&owner), "ghost").unwrap_err();
        assert_eq!(
            err,
            CaretStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Happy path
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r604_fresh_state_reports_disabled_and_hidden() {
        // CaretBlink::new defaults to enabled=false / visible=false.
        let owner = Owner::new();
        let _state = bind_state(&owner, "field");
        let outcome = caret_state(Some(&owner), "field").unwrap();
        assert_eq!(outcome.tag, "field");
        assert!(!outcome.visible);
        assert!(!outcome.enabled);
    }

    #[test]
    fn r604_enabled_state_becomes_visible_immediately() {
        // set_enabled(true) flips visible to true per the
        // canonical "focus reveals caret" UX.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_enabled(true);
        let outcome = caret_state(Some(&owner), "field").unwrap();
        assert!(outcome.visible);
        assert!(outcome.enabled);
    }

    #[test]
    fn r604_period_secs_matches_substrate_constant() {
        // The wire echoes the canonical blink period verbatim.
        let owner = Owner::new();
        let _state = bind_state(&owner, "field");
        let outcome = caret_state(Some(&owner), "field").unwrap();
        assert!(
            (outcome.period_secs - CaretBlink::PERIOD_SECS).abs() < f32::EPSILON,
            "period_secs must equal CaretBlink::PERIOD_SECS",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Side-effect contract
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r604_call_does_not_insert_a_new_cache_slot() {
        // Critical for caret_state specifically: use_caret_blink
        // registers an animation driver with the owner on first
        // resolve. Letting the RPC path lazy-init would grow the
        // tick workload silently. The cache_contains gate must
        // protect against this.
        let owner = Owner::new();
        let _ = caret_state(Some(&owner), "phantom").unwrap_err();
        assert!(
            !owner.cache_contains::<CaretBlink>("phantom"),
            "caret_state must not materialize a CaretBlink on failed lookup",
        );
    }

    #[test]
    fn r604_call_is_idempotent_two_consecutive_snapshots_match() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_enabled(true);
        let a = caret_state(Some(&owner), "field").unwrap();
        let b = caret_state(Some(&owner), "field").unwrap();
        assert_eq!(a, b);
    }

    // ─────────────────────────────────────────────────────────────────
    // JSON serialization shape
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r604_outcome_serializes_to_expected_keys() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_enabled(true);
        let outcome = caret_state(Some(&owner), "field").unwrap();
        let json = serde_json::to_value(outcome).unwrap();
        assert_eq!(json["tag"], "field");
        assert_eq!(json["visible"], true);
        assert_eq!(json["enabled"], true);
        let period = json["period_secs"].as_f64().expect("period is a number");
        assert!((period - f64::from(CaretBlink::PERIOD_SECS)).abs() < 1e-5);
        let obj = json.as_object().expect("outcome is a JSON object");
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let key_strs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        assert_eq!(key_strs, vec!["enabled", "period_secs", "tag", "visible"]);
    }
}
