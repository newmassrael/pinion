//! `scene/cancel_preview` typed dispatcher (§5.34 R40.2).
//!
//! Thin wrapper around [`PreviewLedger::cancel`] surfaced as a typed
//! function so non-JSON-RPC carriers (direct in-process bindings,
//! future gRPC) can call the same logic without re-parsing wire shapes.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) lives in
//! [`crate::dispatch`]; this module exposes the typed dispatcher only.

use super::{PreviewId, PreviewLedger};

/// Remove the preview entry for `id` from `ledger`.
///
/// Returns `true` when the entry was active and removed; `false` when
/// the id is unknown, already applied, already cancelled, or has been
/// swept after deadline expiry. Idempotent — repeated calls on the
/// same handle return `false` after the first.
///
/// Per §5.34 the operation is total: there is no error surface
/// (`cancel` failure mode for an unknown id is a successful `false`
/// return, not an error response).
pub fn cancel_preview(ledger: &PreviewLedger, id: PreviewId) -> bool {
    ledger.cancel(id)
}
