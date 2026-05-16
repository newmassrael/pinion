//! Runtime-side intent collection (§5.20, R18 slice 3).
//!
//! Owns the per-frame queue the runtime walks after each event. The
//! walk descends through the `Scene` tree, drains dirty
//! [`External`](pinion_core::external::External) nodes, and pushes
//! the emitted [`Intent`]s into [`IntentQueue`]. A second pass at the
//! `pinion-rpc` boundary (slice 4) exposes the drained queue through
//! the `scene/intents` JSON-RPC method.
//!
//! Concurrency: v0 is single-consumer, synchronous-poll per §5.20.
//! The `scene/invoke` action channel (R17 §5.15) and the `Intent`
//! emission channel are dual sides of the bidirectional surface;
//! both currently funnel through the UI thread.

use pinion_core::external::External;
use pinion_core::intent::Intent;
use pinion_core::Scene;

/// Per-frame queue of [`Intent`]s drained from the scene's
/// [`External`] nodes.
///
/// `drain` returns the accumulated batch and resets the queue;
/// callers (RPC dispatch, app code) own the resulting `Vec<Intent>`.
#[derive(Debug, Default)]
pub struct IntentQueue {
    items: Vec<Intent>,
}

impl IntentQueue {
    /// Empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Push a single intent. Used by [`walk_scene_and_drain`] sinks
    /// and (during tests) by direct producers.
    pub fn push(&mut self, intent: Intent) {
        self.items.push(intent);
    }

    /// Drain every queued intent in FIFO order and clear the queue.
    pub fn drain(&mut self) -> Vec<Intent> {
        core::mem::take(&mut self.items)
    }

    /// Current queued count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when no intents are pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Walk `scene`, drain every dirty [`External`] node, push the
/// resulting intents into `queue`.
///
/// Recurses through `Scene::Container` children; leaf primitives
/// (`Box`/`Text`/`Path`/`Image`/`Effect`) carry no intents. The
/// `External::is_dirty` short-circuit avoids virtual dispatch into
/// `drain_intents` for nodes with nothing pending.
pub fn walk_scene_and_drain(scene: &mut Scene, queue: &mut IntentQueue) {
    match scene {
        Scene::External(node) => drain_one(node.handle.as_mut(), queue),
        Scene::Container(container) => {
            for child in &mut container.children {
                walk_scene_and_drain(child, queue);
            }
        }
        // Closed primitives never emit intents. The wildcard also
        // absorbs future `#[non_exhaustive]` additions — they default
        // to opting out of the §5.20 channel until a follow-up slice
        // opts them in.
        _ => {}
    }
}

fn drain_one(handle: &mut dyn External, queue: &mut IntentQueue) {
    if !handle.is_dirty() {
        return;
    }
    handle.drain_intents(&mut |intent| queue.push(intent));
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, CountedExternal, External, IntrospectValue,
        RepaintOwner, StubExternal, ThreadOwnership,
    };
    use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect};

    /// Test fixture proving the trait surface without any
    /// state-mutation side effects: a single static `Intent` armed
    /// at construction time, harvested on drain.
    #[derive(Debug)]
    struct ArmedEmitter {
        pending: Vec<Intent>,
    }

    impl ArmedEmitter {
        fn with_one(tag: &'static str) -> Self {
            Self {
                pending: vec![Intent::new_static(tag, IntrospectValue::Null)],
            }
        }
    }

    impl External for ArmedEmitter {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
        }

        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }

        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }

        fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
            for intent in self.pending.drain(..) {
                sink(intent);
            }
        }

        fn is_dirty(&self) -> bool {
            !self.pending.is_empty()
        }
    }

    #[test]
    fn empty_queue_starts_empty_and_drains_empty() {
        let mut q = IntentQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        let drained = q.drain();
        assert!(drained.is_empty());
    }

    #[test]
    fn push_then_drain_returns_items_in_order() {
        let mut q = IntentQueue::new();
        q.push(Intent::new_static("a", IntrospectValue::Null));
        q.push(Intent::new_static("b", IntrospectValue::Null));
        assert_eq!(q.len(), 2);
        let drained = q.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].tag_str(), "a");
        assert_eq!(drained[1].tag_str(), "b");
        assert!(q.is_empty());
    }

    #[test]
    fn walk_drains_a_single_external_at_root() {
        let mut scene =
            Scene::External(ExternalNode::new(Box::new(ArmedEmitter::with_one("x.click"))));
        let mut q = IntentQueue::new();
        walk_scene_and_drain(&mut scene, &mut q);
        let drained = q.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "x.click");
    }

    #[test]
    fn walk_skips_clean_external() {
        // StubExternal opts out of the intent channel: is_dirty()
        // returns false, drain virtual call must not fire.
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let mut q = IntentQueue::new();
        walk_scene_and_drain(&mut scene, &mut q);
        assert!(q.is_empty());
    }

    #[test]
    fn walk_skips_non_external_primitives() {
        let mut scene = Scene::Box(BoxNode::new(0, Rect::default()).with_tag("just_a_box"));
        let mut q = IntentQueue::new();
        walk_scene_and_drain(&mut scene, &mut q);
        assert!(q.is_empty());
    }

    #[test]
    fn walk_recurses_into_container_children() {
        // §5.20 dirty walk covers nested External nodes through
        // Container, so a button buried under a toolbar still emits.
        let inner = Scene::External(ExternalNode::new(Box::new(
            ArmedEmitter::with_one("inner.click"),
        )));
        let mut scene = Scene::Container(ContainerNode::new(vec![inner]).with_tag("toolbar"));
        let mut q = IntentQueue::new();
        walk_scene_and_drain(&mut scene, &mut q);
        let drained = q.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "inner.click");
    }

    #[test]
    fn walk_collects_intents_from_dirty_counted_external() {
        // CountedExternal: intervene queues a `counted.changed` intent;
        // the walk picks it up exactly once and leaves the source
        // clean.
        let mut counted = CountedExternal::new(0);
        counted
            .introspect_mut()
            .unwrap()
            .intervene("count", IntrospectValue::Int(11))
            .unwrap();
        let mut scene = Scene::External(ExternalNode::new(Box::new(counted)));
        let mut q = IntentQueue::new();
        walk_scene_and_drain(&mut scene, &mut q);
        let drained = q.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "counted.changed");
        assert_eq!(drained[0].payload, IntrospectValue::Int(11));

        // Second walk against the same scene now drains nothing
        // because the External was flushed clean.
        let mut q2 = IntentQueue::new();
        walk_scene_and_drain(&mut scene, &mut q2);
        assert!(q2.is_empty());
    }
}
