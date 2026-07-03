//! `scene/intents` RPC method dispatch — R18 §5.20 intent system,
//! ninth typed handler extending the §5.12 set from 8 to 9.
//!
//! Mirrors the [`crate::query`](fn@crate::query) / [`crate::invoke`](fn@crate::invoke) pattern but
//! operates on the dual leg of the bidirectional channel: instead of
//! consuming an action (invoke) or reading a slot (query), this method
//! *drains* whatever intents the scene's [`External`] nodes have
//! emitted since the last poll. Single-consumer, synchronous poll per
//! §5.20 v0.
//!
//! v0 behaviour: every dirty `External` in the scene flushes through
//! [`pinion_runtime::walk_scene_and_drain`]; the resulting batch is
//! returned to the caller and the sources are left clean. No window
//! prefix is required — the call drains the entire scene the
//! dispatcher is bound to. A `path` filter parameter is carry-forward
//! per §5.20.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is handled by
//! [`crate::dispatch`](fn@crate::dispatch); this module exposes the typed dispatcher
//! only.
//!
//! [`External`]: pinion_core::external::External

use pinion_core::Scene;
use pinion_core::intent::Intent;
use pinion_runtime::{IntentQueue, walk_scene_and_drain};

/// Reasons the typed [`drain_intents`] dispatcher can fail.
///
/// v0 has no failure modes — the walk is total over every Scene
/// variant — but the typed wrapper preserves room for future
/// path-filter / window-prefix errors without a wire-shape break.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentsError {}

/// Drain every pending [`Intent`] from `scene`, returning them in the
/// order each `External` emitted them.
///
/// Takes `&mut Scene` because draining mutates the underlying
/// `External` queues (matching the [`crate::rewind`](fn@crate::rewind) / [`crate::invoke`](fn@crate::invoke)
/// signatures). Successive calls against an unchanged scene return an
/// empty `Vec`.
///
/// # Errors
///
/// Reserved — see [`IntentsError`]. v0 always returns `Ok`.
pub fn drain_intents(scene: &mut Scene) -> Result<Vec<Intent>, IntentsError> {
    let mut queue = IntentQueue::new();
    walk_scene_and_drain(scene, &mut queue);
    Ok(queue.drain())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, External, IntrospectValue, StubExternal};
    use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect};

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn empty_scene_returns_empty_batch() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let drained = drain_intents(&mut scene).unwrap();
        assert!(drained.is_empty());
    }

    #[test]
    fn clean_external_returns_empty_batch() {
        let mut scene = counted_scene(0);
        let drained = drain_intents(&mut scene).unwrap();
        assert!(drained.is_empty());
    }

    #[test]
    fn stub_external_returns_empty_batch() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let drained = drain_intents(&mut scene).unwrap();
        assert!(drained.is_empty());
    }

    #[test]
    fn dirty_counted_external_drains_one_intent() {
        let mut counted = CountedExternal::new(0);
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(3))
            .unwrap();
        let mut scene = Scene::External(ExternalNode::new(Box::new(counted)));
        let drained = drain_intents(&mut scene).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "counted.changed");
        assert_eq!(drained[0].payload, IntrospectValue::Int(3));
    }

    #[test]
    fn second_drain_returns_empty_after_flush() {
        let mut counted = CountedExternal::new(0);
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(9))
            .unwrap();
        let mut scene = Scene::External(ExternalNode::new(Box::new(counted)));
        let _ = drain_intents(&mut scene).unwrap();
        let again = drain_intents(&mut scene).unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn container_walks_nested_external() {
        // §5.20 walks the whole tree. A dirty button buried under a
        // toolbar still surfaces through the RPC drain.
        let mut counted = CountedExternal::new(0);
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(1))
            .unwrap();
        let inner = Scene::External(ExternalNode::new(Box::new(counted)));
        let mut scene = Scene::Container(ContainerNode::new(vec![inner]));
        let drained = drain_intents(&mut scene).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "counted.changed");
    }
}
