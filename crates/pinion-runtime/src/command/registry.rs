//! [`HandlerRegistry`] — boot-time `kind` → [`Handler`] map.
//!
//! Maps [`Command::kind`](pinion_core::Command::kind) strings to the
//! handler that should dispatch them. Swappable per §5.23 R27:
//! re-`register` the same `kind` replaces the prior handler — useful
//! for tests, mocks, and runtime feature gates.
//!
//! The registry is sync; futures the handlers construct cross to
//! whichever async runtime the executor (`pinion-rpc` /
//! `pinion-shell`) provides.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use pinion_core::Command;

use super::handler::{Handler, HandlerFuture};

/// Maps [`Command::kind`](pinion_core::Command::kind) tags to
/// registered [`Handler`] instances.
///
/// Backed by a [`BTreeMap`] so iteration order is deterministic
/// (lexicographic by `kind`) — that matters for AI introspection
/// where two snapshots must hash identically.
pub struct HandlerRegistry {
    handlers: BTreeMap<Cow<'static, str>, Arc<dyn Handler>>,
}

impl HandlerRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: BTreeMap::new(),
        }
    }

    /// Register (or replace) a [`Handler`] for `kind`. Returns the
    /// prior handler if one was registered for the same kind — useful
    /// for tests that want to restore a baseline registration after
    /// shadowing it.
    pub fn register(
        &mut self,
        kind: impl Into<Cow<'static, str>>,
        handler: Arc<dyn Handler>,
    ) -> Option<Arc<dyn Handler>> {
        self.handlers.insert(kind.into(), handler)
    }

    /// Remove the handler for `kind`, returning it if registered.
    pub fn unregister(&mut self, kind: &str) -> Option<Arc<dyn Handler>> {
        self.handlers.remove(kind)
    }

    /// Dispatch a [`Command`] — looks up the handler for
    /// [`Command::kind_str`](pinion_core::Command::kind_str) and
    /// constructs the [`HandlerFuture`].
    ///
    /// Returns [`None`] when no handler is registered for the kind —
    /// callers decide whether to log, drop, or surface as an error.
    /// The future itself is not driven here; the caller's executor
    /// polls it to completion.
    #[must_use]
    pub fn dispatch(&self, command: Command) -> Option<HandlerFuture> {
        let handler = self.handlers.get(command.kind_str())?;
        Some(handler.handle(command))
    }

    /// `true` when a handler is registered for `kind`.
    #[must_use]
    pub fn has(&self, kind: &str) -> bool {
        self.handlers.contains_key(kind)
    }

    /// Iterator over registered kinds, lexicographic order.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.handlers.keys().map(Cow::as_ref)
    }

    /// Number of registered handlers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// `true` when the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures_executor::block_on;
    use pinion_core::external::IntrospectValue;
    use pinion_core::{Command, Intent};

    use super::{Handler, HandlerFuture, HandlerRegistry};

    fn echo_handler() -> Arc<dyn Handler> {
        Arc::new(|cmd: Command| -> HandlerFuture {
            Box::pin(async move {
                Intent::new_owned(format!("echo.{}", cmd.kind_str()), cmd.payload)
            })
        })
    }

    fn const_intent_handler(tag: &'static str) -> Arc<dyn Handler> {
        Arc::new(move |_cmd: Command| -> HandlerFuture {
            Box::pin(async move { Intent::new_static(tag, IntrospectValue::Null) })
        })
    }

    #[test]
    fn new_is_empty() {
        let registry = HandlerRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.kinds().count(), 0);
    }

    #[test]
    fn default_equals_new() {
        let registry = HandlerRegistry::default();
        assert!(registry.is_empty());
    }

    #[test]
    fn register_then_has() {
        let mut registry = HandlerRegistry::new();
        assert!(!registry.has("http.get"));
        let prior = registry.register("http.get", echo_handler());
        assert!(prior.is_none());
        assert!(registry.has("http.get"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn register_replaces_returns_prior() {
        let mut registry = HandlerRegistry::new();
        registry.register("audio.play", const_intent_handler("first"));
        let prior = registry.register("audio.play", const_intent_handler("second"));
        assert!(prior.is_some(), "second register returns prior handler");
        assert_eq!(registry.len(), 1, "replacement does not grow the map");
    }

    #[test]
    fn dispatch_routes_by_kind() {
        let mut registry = HandlerRegistry::new();
        registry.register("http.get", echo_handler());
        let cmd = Command::new_static("http.get", IntrospectValue::Text("/x".into()), 9);
        let future = registry.dispatch(cmd).expect("kind registered");
        let intent = block_on(future);
        assert_eq!(intent.tag_str(), "echo.http.get");
        assert_eq!(intent.payload, IntrospectValue::Text("/x".into()));
    }

    #[test]
    fn dispatch_unknown_kind_returns_none() {
        let registry = HandlerRegistry::new();
        let cmd = Command::new_static("nope", IntrospectValue::Null, 0);
        assert!(registry.dispatch(cmd).is_none());
    }

    #[test]
    fn unregister_returns_handler_and_clears() {
        let mut registry = HandlerRegistry::new();
        registry.register("clipboard.write", echo_handler());
        let removed = registry.unregister("clipboard.write");
        assert!(removed.is_some());
        assert!(!registry.has("clipboard.write"));
        assert!(registry.unregister("clipboard.write").is_none());
    }

    #[test]
    fn kinds_iter_is_lexicographic() {
        let mut registry = HandlerRegistry::new();
        registry.register("z.last", echo_handler());
        registry.register("a.first", echo_handler());
        registry.register("m.middle", echo_handler());
        let kinds: Vec<&str> = registry.kinds().collect();
        assert_eq!(kinds, vec!["a.first", "m.middle", "z.last"]);
    }

    #[test]
    fn registry_dispatches_multiple_kinds() {
        let mut registry = HandlerRegistry::new();
        registry.register("a", const_intent_handler("intent.a"));
        registry.register("b", const_intent_handler("intent.b"));
        let cmd_a = Command::new_static("a", IntrospectValue::Null, 0);
        let cmd_b = Command::new_static("b", IntrospectValue::Null, 0);
        assert_eq!(block_on(registry.dispatch(cmd_a).unwrap()).tag_str(), "intent.a");
        assert_eq!(block_on(registry.dispatch(cmd_b).unwrap()).tag_str(), "intent.b");
    }

    #[test]
    fn registry_with_owned_kind_works_too() {
        let mut registry = HandlerRegistry::new();
        let dynamic_kind = format!("plugin.{}", "x");
        registry.register(dynamic_kind, const_intent_handler("plugin.fired"));
        let cmd = Command::new_static("plugin.x", IntrospectValue::Null, 0);
        let future = registry.dispatch(cmd).expect("owned kind matches");
        assert_eq!(block_on(future).tag_str(), "plugin.fired");
    }

    #[test]
    fn arc_handler_can_be_shared_between_registries() {
        let handler = echo_handler();
        let mut registry_a = HandlerRegistry::new();
        let mut registry_b = HandlerRegistry::new();
        registry_a.register("shared", Arc::clone(&handler));
        registry_b.register("shared", handler);
        let cmd = Command::new_static("shared", IntrospectValue::Int(7), 0);
        let intent_a = block_on(registry_a.dispatch(cmd.clone()).unwrap());
        let intent_b = block_on(registry_b.dispatch(cmd).unwrap());
        assert_eq!(intent_a, intent_b);
    }
}
