//! [`ViewBlueprint`] — declarative scene description used by
//! [`TypedProposal::ReplaceView`](super::TypedProposal) (§5.34 R40.11).
//!
//! A `Scene` value is intentionally `!Send + !Sync + !Clone`
//! (`ExternalNode` carries `Box<dyn External>` without those bounds —
//! [`Scene`] doc). The preview ledger stores `Box<dyn Proposal>` and
//! [`Proposal`](super::Proposal) requires `Send + Sync + 'static`, so
//! a proposal variant **cannot** carry a `Scene` directly.
//!
//! `ViewBlueprint` is the textbook bridge: a closed-form, `Send +
//! Sync + Clone` description that materialises into a `Scene` exactly
//! once at apply time via [`ViewBlueprint::materialize`]. v0 covers
//! the two introspectable primitives that anchor every other tag-
//! addressable widget in the framework — `Box` and `Container` —
//! which together let the AI agent swap any tagged scene region for
//! a fresh sub-tree of styled boxes. `Text`, `Path`, `Image`, and
//! the two opaque escapes (`Effect`, `External`) land as additive
//! variants in subsequent R40.x sub-slices.
//!
//! `#[non_exhaustive]` so every later variant addition stays a
//! non-breaking enum extension per Bloch / Hyrum.

use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::BoxStyle;
use pinion_core::Scene;

/// Wire-form scene description carried by
/// [`TypedProposal::ReplaceView`](super::TypedProposal). Clone-friendly
/// so the surrounding `TypedProposal` enum keeps its `Clone` derive;
/// `Send + Sync` so [`Proposal`](super::Proposal)'s bounds hold.
///
/// Materialise into a concrete [`Scene`] by consuming with
/// [`ViewBlueprint::materialize`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewBlueprint {
    /// Single styled rectangle (no children). Mirrors the v0 fields
    /// of [`pinion_core::scene::BoxNode`]; `layout` / future style
    /// extensions arrive as additive blueprint enrichments.
    Box {
        /// Absolute pixel rect in the same `u32` coordinate frame
        /// the rest of the scene uses.
        rect: Rect,
        /// Fill / border / corner-radius. Reuse of the runtime
        /// [`BoxStyle`] keeps wire-to-materialised parity exact.
        style: BoxStyle,
        /// §5.20 intent tag (when set).
        tag: Option<String>,
    },
    /// Layout container holding zero or more nested blueprints.
    /// `style` populates the container's own background (R24 slice 5).
    Container {
        rect: Rect,
        style: BoxStyle,
        tag: Option<String>,
        children: Vec<ViewBlueprint>,
    },
}

impl ViewBlueprint {
    /// Consume this blueprint and produce the matching [`Scene`]
    /// subtree. Tag strings are owned in the blueprint and move into
    /// the materialised node's tag slot; no allocation copy of the
    /// children vector beyond the per-element materialise calls.
    #[must_use]
    pub fn materialize(self) -> Scene {
        match self {
            Self::Box { rect, style, tag } => {
                let mut node = BoxNode::new(rect, style);
                node.tag = tag.map(Into::into);
                Scene::Box(node)
            }
            Self::Container {
                rect,
                style,
                tag,
                children,
            } => {
                let materialised: Vec<Scene> =
                    children.into_iter().map(ViewBlueprint::materialize).collect();
                let mut node = ContainerNode::new(materialised);
                node.rect = rect;
                node.style = style;
                node.tag = tag.map(Into::into);
                Scene::Container(node)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::Color;

    #[test]
    fn materialize_box_preserves_rect_style_and_tag() {
        let bp = ViewBlueprint::Box {
            rect: Rect::new(10, 20, 30, 40),
            style: BoxStyle::filled(Color::from_argb(0x00ab_cdef)),
            tag: Some("save_btn".to_string()),
        };
        let scene = bp.materialize();
        match scene {
            Scene::Box(b) => {
                assert_eq!(b.rect, Rect::new(10, 20, 30, 40));
                assert_eq!(b.style.fill, Color::from_argb(0x00ab_cdef));
                assert_eq!(b.tag.as_deref(), Some("save_btn"));
            }
            _ => panic!("expected Box"),
        }
    }

    #[test]
    fn materialize_container_with_children_preserves_structure() {
        let bp = ViewBlueprint::Container {
            rect: Rect::new(0, 0, 100, 100),
            style: BoxStyle::filled(Color::from_argb(0x0011_1111)),
            tag: Some("panel".to_string()),
            children: vec![
                ViewBlueprint::Box {
                    rect: Rect::new(0, 0, 10, 10),
                    style: BoxStyle::default(),
                    tag: None,
                },
                ViewBlueprint::Box {
                    rect: Rect::new(20, 20, 10, 10),
                    style: BoxStyle::default(),
                    tag: Some("inner".to_string()),
                },
            ],
        };
        let scene = bp.materialize();
        match scene {
            Scene::Container(c) => {
                assert_eq!(c.tag.as_deref(), Some("panel"));
                assert_eq!(c.children.len(), 2);
                assert_eq!(c.children[0].tag(), None);
                assert_eq!(c.children[1].tag(), Some("inner"));
            }
            _ => panic!("expected Container"),
        }
    }

    #[test]
    fn materialize_empty_container_works() {
        let bp = ViewBlueprint::Container {
            rect: Rect::new(0, 0, 50, 50),
            style: BoxStyle::default(),
            tag: None,
            children: vec![],
        };
        let scene = bp.materialize();
        match scene {
            Scene::Container(c) => {
                assert!(c.children.is_empty());
                assert_eq!(c.rect, Rect::new(0, 0, 50, 50));
            }
            _ => panic!("expected Container"),
        }
    }

    #[test]
    fn nested_container_chain_materializes_recursively() {
        let inner = ViewBlueprint::Container {
            rect: Rect::new(0, 0, 50, 50),
            style: BoxStyle::default(),
            tag: Some("inner".to_string()),
            children: vec![ViewBlueprint::Box {
                rect: Rect::new(5, 5, 10, 10),
                style: BoxStyle::default(),
                tag: Some("leaf".to_string()),
            }],
        };
        let outer = ViewBlueprint::Container {
            rect: Rect::new(0, 0, 100, 100),
            style: BoxStyle::default(),
            tag: Some("outer".to_string()),
            children: vec![inner],
        };
        let scene = outer.materialize();
        match scene {
            Scene::Container(o) => match &o.children[0] {
                Scene::Container(i) => {
                    assert_eq!(i.tag.as_deref(), Some("inner"));
                    assert_eq!(i.children[0].tag(), Some("leaf"));
                }
                _ => panic!("inner not Container"),
            },
            _ => panic!("outer not Container"),
        }
    }

    #[test]
    fn blueprint_is_clone_and_send_sync() {
        // Compile-time guards: ViewBlueprint must be Clone (the
        // surrounding TypedProposal::Clone derive depends on it) and
        // Send + Sync (Proposal trait bound).
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_clone<T: Clone>() {}
        assert_send_sync::<ViewBlueprint>();
        assert_clone::<ViewBlueprint>();
    }
}
