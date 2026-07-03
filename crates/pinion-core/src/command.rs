//! §5.23 — declarative `Command` substrate (R51.139 first-cut).
//!
//! A [`Command`] is the Elm/Iced declarative async/IO description that
//! mirrors [`Intent`](crate::intent::Intent) on the outbound side:
//! widgets and reducers produce a [`Command`] to *describe* what should
//! happen ("fetch this URL", "open this dialog", "ring the bell") and
//! the framework (or a swappable handler registry) executes it —
//! returning back into the [`Intent`](crate::intent::Intent) flow.
//!
//! ## What lands at R51.139
//!
//! The synchronous **declaration** + **queueing** half of the contract:
//!
//! - [`Command`] wire-form struct (`kind` tag + `IntrospectValue` payload
//!   + scope id), Serialize-friendly for RPC inspection.
//! - Owner-tied pending queue (see
//!   [`Owner::dispatch_command`](crate::reactive::Owner::dispatch_command)
//!   / [`Owner::take_pending_commands`](crate::reactive::Owner::take_pending_commands)
//!   / [`Owner::pending_commands`](crate::reactive::Owner::pending_commands)).
//! - Cancellation via Owner drop — pending commands attached to a
//!   dropped scope evaporate with it, the textbook Solid.js pattern.
//! - `dry_run` / scenario exploration: [`Owner::pending_commands`](crate::reactive::Owner::pending_commands) returns
//!   a snapshot for inspection without executing or consuming the queue.
//!
//! ## What carries (R51.140+)
//!
//! - `Handler` trait + boot-time registry (async dispatch surface). The
//!   contract is `async fn handle(Command) -> Intent`; storage of futures
//!   forces a `tokio` (or generic executor) dep, which belongs in
//!   `pinion-runtime` / `pinion-rpc`, not `pinion-core` (§6.3 async
//!   model: view-fn sync, IO async at the boundary).
//! - `scene/commands` RPC method (10th method) — surfaces the pending
//!   queue snapshot to AI agents for in-flight inspection (§5.7).
//! - `Update(&mut Model, Intent) -> Vec<Command>` signature evolution
//!   — bundles the framework's reducer contract with the `Command`
//!   producer side once `Handler` lands.
//! - SCE schema for declarative command tables + handler bindings; Forge
//!   emit. Mirrors `pinion-forge` intent/signal codegen.
//!
//! ## Why mirror `Intent`?
//!
//! Two-way symmetry: `Intent` is the **inbound** symbolic channel
//! (widget → framework / AI agent), `Command` is the **outbound** one
//! (framework / agent → IO). Sharing the wire shape (`Cow` tag +
//! `IntrospectValue` payload) means the same RPC inspection plumbing
//! works on both — `scene/intents` and `scene/commands` walk identical
//! structures.

use std::borrow::{Borrow, Cow};

use crate::external::IntrospectValue;

/// Wire-form declarative IO/async description.
///
/// Produced by reducers (R27 caveat: `Update(&mut Model, Intent) ->
/// Vec<Command>`) or by agents through the future
/// [`scene/commands` RPC method](crate::reactive::Owner::pending_commands).
/// The framework (or registered `Handler` — carry: R51.140+) consumes
/// these by draining the owner's queue via
/// [`Owner::take_pending_commands`](crate::reactive::Owner::take_pending_commands).
///
/// # Fields
///
/// - `kind` — symbolic discriminator, by convention
///   `<feature>.<operation>` (e.g. `http.get`, `clipboard.write`,
///   `audio.play`). [`Cow`] keeps the static-string emission path
///   zero-alloc.
/// - `payload` — typed argument matching the operation. Uses
///   [`IntrospectValue`] for the same reason as [`Intent`](crate::intent::Intent):
///   RPC inspection over JSON-RPC needs a Serialize-friendly value
///   surface.
/// - `scope_id` — the [`Owner`](crate::reactive::Owner) id that
///   produced this command. Carried on the wire so RPC inspection /
///   `dry_run` can attribute pending IO to a scope; on Owner drop the
///   scope's queue evaporates so dangling work is impossible.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub kind: Cow<'static, str>,
    pub payload: IntrospectValue,
    pub scope_id: u64,
}

impl Command {
    /// Construct a command with a static kind string.
    #[must_use]
    pub const fn new_static(kind: &'static str, payload: IntrospectValue, scope_id: u64) -> Self {
        Self {
            kind: Cow::Borrowed(kind),
            payload,
            scope_id,
        }
    }

    /// Construct a command with an owned kind string. Use the static
    /// variant when the kind tag is a literal.
    #[must_use]
    pub fn new_owned(kind: String, payload: IntrospectValue, scope_id: u64) -> Self {
        Self {
            kind: Cow::Owned(kind),
            payload,
            scope_id,
        }
    }

    /// Kind tag as a `&str`.
    #[must_use]
    pub fn kind_str(&self) -> &str {
        self.kind.borrow()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_static_preserves_fields() {
        let cmd = Command::new_static("http.get", IntrospectValue::Text("/api/v1".to_string()), 7);
        assert_eq!(cmd.kind_str(), "http.get");
        assert_eq!(cmd.payload, IntrospectValue::Text("/api/v1".to_string()));
        assert_eq!(cmd.scope_id, 7);
    }

    #[test]
    fn new_owned_accepts_dynamic_kind() {
        let kind = format!("file.read.{}", "scratch");
        let cmd = Command::new_owned(kind, IntrospectValue::Null, 3);
        assert_eq!(cmd.kind_str(), "file.read.scratch");
    }

    #[test]
    fn equality_is_structural() {
        let a = Command::new_static("audio.play", IntrospectValue::Int(440), 1);
        let b = Command::new_static("audio.play", IntrospectValue::Int(440), 1);
        assert_eq!(a, b);
        let c = Command::new_static("audio.play", IntrospectValue::Int(441), 1);
        assert_ne!(a, c);
    }

    #[test]
    fn clone_preserves_all_fields() {
        let cmd = Command::new_static("clipboard.write", IntrospectValue::Text("x".to_string()), 9);
        let alias = cmd.clone();
        assert_eq!(cmd, alias);
    }
}
