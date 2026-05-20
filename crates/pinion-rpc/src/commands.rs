//! `scene/commands` RPC method dispatch — §5.23 R27 + §5.7 10th method.
//!
//! Sibling of [`crate::intents`] but on the outbound dispatch leg:
//! lists every pending [`Command`](pinion_core::Command) queued on
//! the framework's reactive scopes so AI agents can inspect the
//! dispatch surface without forcing the framework pump to drain.
//!
//! ## Wire shape (R51.161 first-cut)
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "pending": [
//!       {
//!         "kind": "http.get",
//!         "payload": { "Text": "/api/v1" },
//!         "scope_id": 42
//!       },
//!       { "kind": "audio.play", "payload": "Null", "scope_id": 7 }
//!     ]
//!   }
//! }
//! ```
//!
//! ## What lands at R51.161 (this round)
//!
//! - Snapshot of *pending* commands from [`pinion_core::Owner`]
//!   sub-trees ([`Owner::pending_commands_recursive`](pinion_core::reactive::Owner::pending_commands_recursive)
//!   — non-draining peek). Children depth-first then root, matching
//!   the framework pump's traversal so the AI agent sees the
//!   commands in the order they would be dispatched.
//!
//! ## What carries (R51.162+)
//!
//! - **In-flight introspection** — extend
//!   `pinion_runtime::CommandExecutor.in_flight` to track
//!   `(kind, payload)` alongside the
//!   [`CommandTaskHandle`](pinion_runtime::CommandTaskHandle), then
//!   surface `result.in_flight: [...]` alongside the pending list.
//!   Currently the executor only retains the cancel handle keyed by
//!   `scope_id`, which is insufficient for introspection.
//! - **Path filter** — same shape as `scene/intents`'s carry, applies
//!   here too once the §5.20 path filter axis lands.

use pinion_core::Command;
use pinion_core::Owner;
use serde::Serialize;

use crate::dispatch::introspect_value_to_json;

/// Reasons the typed [`list_pending_commands`] dispatcher can fail.
///
/// v0 has no failure modes — the snapshot walk is total over the
/// owner tree — but the typed wrapper preserves room for future
/// path-filter / scope-filter errors without a wire-shape break.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandsError {}

/// R51.161 §5.23 — JSON-RPC `result.pending[*]` element shape.
///
/// Renamed from [`Command`] to a distinct view-side type so a future
/// in-flight projection (`result.in_flight[*]`, R51.162 carry) can
/// share the same fields without coupling the substrate's
/// [`Command`] struct to a JSON-RPC contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingCommandView {
    /// [`Command::kind_str`] — the symbolic discriminator
    /// (`<feature>.<operation>`).
    pub kind: String,
    /// [`Command::payload`] rendered through the same
    /// `IntrospectValue → JSON` mapping the other RPC methods use.
    pub payload: serde_json::Value,
    /// [`Command::scope_id`] — the [`Owner`] id that produced the
    /// command. AI agents use this to correlate with the
    /// `scene/snapshot` widget tags (carry: scope-id → tag lookup).
    pub scope_id: u64,
}

/// Snapshot every pending [`Command`] on `owner` and its descendants
/// into the [`PendingCommandView`] wire shape.
///
/// Non-draining: the underlying queues stay populated so the
/// framework pump can drain them on the next dispatch cycle. The
/// resulting `Vec` is in [`Owner::pending_commands_recursive`]
/// traversal order (children depth-first then root) so the JSON-RPC
/// response mirrors the order the framework would dispatch.
///
/// # Errors
///
/// Reserved — see [`CommandsError`]. v0 always returns `Ok`.
pub fn list_pending_commands(owner: &Owner) -> Result<Vec<PendingCommandView>, CommandsError> {
    let snapshot = owner.pending_commands_recursive();
    Ok(snapshot.into_iter().map(command_to_view).collect())
}

fn command_to_view(cmd: Command) -> PendingCommandView {
    PendingCommandView {
        kind: cmd.kind_str().to_string(),
        payload: introspect_value_to_json(cmd.payload),
        scope_id: cmd.scope_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;

    #[test]
    fn empty_owner_returns_empty_pending_list() {
        let owner = Owner::new();
        let pending = list_pending_commands(&owner).unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_single_command_round_trips() {
        let owner = Owner::new();
        owner.dispatch_command(Command::new_static(
            "http.get",
            IntrospectValue::Text("/api/v1".into()),
            owner.id(),
        ));
        let pending = list_pending_commands(&owner).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "http.get");
        assert_eq!(pending[0].scope_id, owner.id());
        assert_eq!(
            pending[0].payload,
            serde_json::Value::String("/api/v1".into()),
        );
    }

    #[test]
    fn pending_traversal_is_children_first_then_root() {
        let parent = Owner::new();
        let child = Owner::new_child(&parent);
        parent.dispatch_command(Command::new_static("p", IntrospectValue::Null, parent.id()));
        child.dispatch_command(Command::new_static("c", IntrospectValue::Null, child.id()));
        let pending = list_pending_commands(&parent).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].kind, "c");
        assert_eq!(pending[1].kind, "p");
    }

    #[test]
    fn list_does_not_drain_underlying_queues() {
        let owner = Owner::new();
        owner.dispatch_command(Command::new_static(
            "x",
            IntrospectValue::Null,
            owner.id(),
        ));
        let _peek_a = list_pending_commands(&owner).unwrap();
        let _peek_b = list_pending_commands(&owner).unwrap();
        let drained = owner.take_pending_commands_recursive();
        assert_eq!(drained.len(), 1, "snapshot must not consume the queue");
    }

    #[test]
    fn pending_command_view_serializes_to_expected_json_shape() {
        let view = PendingCommandView {
            kind: "audio.play".into(),
            payload: serde_json::Value::Number(440.into()),
            scope_id: 7,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["kind"], "audio.play");
        assert_eq!(json["payload"], 440);
        assert_eq!(json["scope_id"], 7);
    }

    #[test]
    fn introspect_value_text_payload_maps_to_string() {
        let owner = Owner::new();
        owner.dispatch_command(Command::new_owned(
            "clipboard.write".into(),
            IntrospectValue::Text("hello".into()),
            owner.id(),
        ));
        let pending = list_pending_commands(&owner).unwrap();
        assert_eq!(pending[0].payload, serde_json::Value::String("hello".into()));
    }

    #[test]
    fn introspect_value_int_payload_maps_to_number() {
        let owner = Owner::new();
        owner.dispatch_command(Command::new_static(
            "k",
            IntrospectValue::Int(123),
            owner.id(),
        ));
        let pending = list_pending_commands(&owner).unwrap();
        assert_eq!(pending[0].payload, serde_json::Value::Number(123.into()));
    }

    #[test]
    fn introspect_value_bool_payload_maps_to_bool() {
        let owner = Owner::new();
        owner.dispatch_command(Command::new_static(
            "k",
            IntrospectValue::Bool(true),
            owner.id(),
        ));
        let pending = list_pending_commands(&owner).unwrap();
        assert_eq!(pending[0].payload, serde_json::Value::Bool(true));
    }
}
