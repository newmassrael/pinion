//! [`ViewBlueprint`] — declarative scene description used by
//! [`TypedProposal::ReplaceView`](super::TypedProposal) (§5.34 R40.11
//! / R43).
//!
//! `ViewBlueprint` is the **wire-side description** of a scene
//! subtree, not a parallel runtime representation. The two surfaces
//! play different roles:
//!
//!   * [`Scene`] — runtime tree consumed by the renderer / RPC layer.
//!     `!Send + !Sync + !Clone` because `ExternalNode` owns a
//!     `Box<dyn External>` without those bounds.
//!   * [`ViewBlueprint`] — JSON-RPC payload shape (`Send + Sync +
//!     Clone`) carried inside `TypedProposal::ReplaceView`.
//!     Materialises into a `Scene` exactly once at apply time via
//!     [`ViewBlueprint::materialize`].
//!
//! Bloch "value objects" / Hickey "data is the API" pattern: a wire
//! description owned by the wire surface, distinct from the runtime
//! representation by design — not a workaround.
//!
//! ## Why a blueprint may carry a bare `rect` (R1345 §5.21)
//!
//! Every variant here states its geometry as a [`Rect`] and materialises into a
//! node with a **default `LayoutStyle`**. Since R1344 that is normally a bug:
//! `compute_layout` overwrites `rect` — it is an OUTPUT — so geometry authored
//! there never reaches a pixel, and a view must state its intent in
//! `LayoutStyle` instead.
//!
//! It is sound here for one reason, and only that reason:
//! [`TypedProposal::ReplaceView`](super::TypedProposal) splices the materialised
//! subtree into `DispatchContext::scene` — the **state** scene — and **no code
//! path lays that scene out**. Both shells run `compute_layout` exclusively over
//! the freshly-produced *paint* scene (`ShellCore::compute_paint_scene` /
//! `ShellCoreTui::compute_paint_scene` lay out `V::view`'s result, never
//! `CoreShell::scene_mut`). So a blueprint's rects survive verbatim and are the
//! authoritative geometry for what reads them (`scene/query`, `scene/snapshot`
//! `from:state`) — the same footing as a binding that runs no layout at all.
//!
//! **The constraint this rests on**: if a materialised blueprint is ever spliced
//! into a scene that DOES run `compute_layout` — a paint scene, or a state scene
//! that someone later decides to lay out — every `rect` here silently becomes
//! dead intent and the subtree collapses to the block-flow default. At that
//! point the wire form must carry layout intent (the v0 shape deliberately
//! carries no layout-mode hints; widening it is the follow-up, not a workaround
//! at the splice site). Verified at R1345, not assumed: the R1344/R1345 rounds
//! exist because exactly this invariant was left undocumented elsewhere and a
//! later round read the absence as a design.
//!
//! ## `Path` commands are rect-relative (R1358)
//!
//! [`ViewBlueprint::Path`]'s `commands` follow the [`PathNode`] contract:
//! they are relative to that variant's own `rect`, not window
//! coordinates. Nothing changed at
//! R1358 — this materialises whatever the wire sent — but what a client
//! must *send* did: a blueprint path is authored in its own box and placed
//! by its `rect`. Recorded because the round that flips a wire form's
//! meaning is the round that must say so; the pass-through would otherwise
//! read as unaffected.
//!
//! ## Variant coverage (R43)
//!
//! R40.11 landed `Box` + `Container`. R43 adds the remaining
//! introspectable variants — `Text` / `Path` / `Image` — for parity
//! with [`Scene`]'s closed-form primitives. The two opaque escapes
//! are intentionally **excluded** from the wire surface:
//!
//!   * `Scene::Effect` — opaque shader / GPU effect. No declarative
//!     wire shape; embedding requires platform-specific shader bytes
//!     out of scope for the JSON-RPC boundary.
//!   * `Scene::External` — opaque embedded content (`Box<dyn
//!     External>`). The author-side handle is unknown at wire-decode
//!     time; injecting a new External via RPC needs a separate
//!     primitive (future R-axis: per-External factory registry).
//!
//! Wire payloads asking for `kind: "Effect"` / `kind: "External"`
//! surface as `Invalid params` at the [`dispatch`](fn@crate::dispatch)
//! boundary — the unsupported-kinds list is closed by design here.
//!
//! `#[non_exhaustive]` so every later variant addition stays a
//! non-breaking enum extension per Bloch / Hyrum.

use pinion_core::Scene;
use pinion_core::scene::{
    BoxNode, ContainerNode, ImageNode, PathCommand, PathNode, Rect, TextNode,
};
use pinion_core::style::{BoxStyle, ImageStyle, PathStyle, TextStyle};

/// Wire-form scene description carried by
/// [`TypedProposal::ReplaceView`](super::TypedProposal). Clone-friendly
/// so the surrounding `TypedProposal` enum keeps its `Clone` derive;
/// `Send + Sync` so [`Proposal`](super::Proposal)'s bounds hold.
///
/// Materialise into a concrete [`Scene`] by consuming with
/// [`ViewBlueprint::materialize`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ViewBlueprint {
    /// Single styled rectangle (no children). Mirrors the v0 fields
    /// of [`BoxNode`].
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
    /// Text primitive — `content` is the raw string payload; cosmic-
    /// text rasterisation happens at render time per `style` (§5.3
    /// R20). v0 carries no layout-mode hints; future R-axes layer
    /// those on as additive blueprint enrichments.
    Text {
        content: String,
        rect: Rect,
        style: TextStyle,
        tag: Option<String>,
    },
    /// Vector path — `commands` is the structured command stream the
    /// rasterizer consumes (§5.3 R20). `style` carries stroke / fill
    /// per [`PathStyle`].
    Path {
        commands: Vec<PathCommand>,
        rect: Rect,
        style: PathStyle,
        tag: Option<String>,
    },
    /// Raster or vector image — `source` is the opaque locator
    /// (`file://`, `https://`, `memory://...`); the framework does
    /// not interpret the URI scheme. `style` carries the fit policy
    /// and optional tint per [`ImageStyle`].
    Image {
        source: String,
        rect: Rect,
        style: ImageStyle,
        tag: Option<String>,
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
                let materialised: Vec<Scene> = children
                    .into_iter()
                    .map(ViewBlueprint::materialize)
                    .collect();
                let mut node = ContainerNode::new(materialised);
                node.rect = rect;
                node.style = style;
                node.tag = tag.map(Into::into);
                Scene::Container(node)
            }
            Self::Text {
                content,
                rect,
                style,
                tag,
            } => {
                let mut node = TextNode::styled(content, rect, style);
                node.tag = tag.map(Into::into);
                Scene::Text(node)
            }
            Self::Path {
                commands,
                rect,
                style,
                tag,
            } => {
                let mut node = PathNode::new(rect, commands, style);
                node.tag = tag.map(Into::into);
                Scene::Path(node)
            }
            Self::Image {
                source,
                rect,
                style,
                tag,
            } => {
                let mut node = ImageNode::styled(source, rect, style);
                node.tag = tag.map(Into::into);
                Scene::Image(node)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{PathCommand, PathPoint};
    use pinion_core::style::{Color, Fit};

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

    // ---- §5.34 R43: Text / Path / Image parity ----

    #[test]
    fn materialize_text_preserves_content_rect_style_and_tag() {
        let bp = ViewBlueprint::Text {
            content: "Save".to_string(),
            rect: Rect::new(60, 80, 140, 60),
            style: TextStyle::new(),
            tag: Some("save_label".to_string()),
        };
        let scene = bp.materialize();
        match scene {
            Scene::Text(t) => {
                assert_eq!(t.content, "Save");
                assert_eq!(t.rect, Rect::new(60, 80, 140, 60));
                assert_eq!(t.tag.as_deref(), Some("save_label"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn materialize_path_preserves_commands_rect_style_and_tag() {
        let commands = vec![
            PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
            PathCommand::LineTo(PathPoint::new(10.0, 10.0)),
            PathCommand::Close,
        ];
        let bp = ViewBlueprint::Path {
            commands: commands.clone(),
            rect: Rect::new(0, 0, 32, 32),
            style: PathStyle::filled(Color::from_argb(0x00ff_ffff)),
            tag: Some("logo".to_string()),
        };
        let scene = bp.materialize();
        match scene {
            Scene::Path(p) => {
                assert_eq!(p.commands, commands);
                assert_eq!(p.rect, Rect::new(0, 0, 32, 32));
                assert_eq!(p.tag.as_deref(), Some("logo"));
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn materialize_image_preserves_source_rect_style_and_tag() {
        let bp = ViewBlueprint::Image {
            source: "file:///tmp/icon.png".to_string(),
            rect: Rect::new(8, 8, 24, 24),
            style: ImageStyle::default().with_fit(Fit::Contain),
            tag: Some("avatar".to_string()),
        };
        let scene = bp.materialize();
        match scene {
            Scene::Image(i) => {
                assert_eq!(i.source, "file:///tmp/icon.png");
                assert_eq!(i.rect, Rect::new(8, 8, 24, 24));
                assert_eq!(i.style.fit, Fit::Contain);
                assert_eq!(i.tag.as_deref(), Some("avatar"));
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn container_can_hold_mixed_variants() {
        // R43: parity check — Container child list accepts every
        // closed-form variant (Box/Container/Text/Path/Image) so the
        // AI agent can express any introspectable subtree.
        let bp = ViewBlueprint::Container {
            rect: Rect::new(0, 0, 100, 100),
            style: BoxStyle::default(),
            tag: None,
            children: vec![
                ViewBlueprint::Box {
                    rect: Rect::default(),
                    style: BoxStyle::default(),
                    tag: None,
                },
                ViewBlueprint::Text {
                    content: "hi".to_string(),
                    rect: Rect::default(),
                    style: TextStyle::new(),
                    tag: None,
                },
                ViewBlueprint::Path {
                    commands: vec![],
                    rect: Rect::default(),
                    style: PathStyle::default(),
                    tag: None,
                },
                ViewBlueprint::Image {
                    source: "memory://0xABCD".to_string(),
                    rect: Rect::default(),
                    style: ImageStyle::default(),
                    tag: None,
                },
            ],
        };
        let scene = bp.materialize();
        match scene {
            Scene::Container(c) => {
                assert_eq!(c.children.len(), 4);
                assert!(matches!(c.children[0], Scene::Box(_)));
                assert!(matches!(c.children[1], Scene::Text(_)));
                assert!(matches!(c.children[2], Scene::Path(_)));
                assert!(matches!(c.children[3], Scene::Image(_)));
            }
            _ => panic!("expected Container"),
        }
    }

    /// R1345 §5.21 — the constraint the bare `rect`s rest on, made executable.
    ///
    /// A blueprint's geometry is authoritative ONLY because `ReplaceView`
    /// splices it into the state scene, which no code path lays out (see the
    /// module docs). This test lays a materialised blueprint out on purpose and
    /// asserts the rects DIE — so the day someone routes a blueprint into a
    /// laid-out scene, or teaches the state scene to lay out, this fails and
    /// says why instead of the subtree silently collapsing to block-flow.
    ///
    /// It is a *documentation* test: the assertion is the hazard, not a wish.
    #[test]
    fn r1345_materialised_rects_do_not_survive_a_layout_pass() {
        use pinion_runtime::{LayoutCache, compute_layout};

        let bp = ViewBlueprint::Container {
            rect: Rect::new(0, 0, 400, 200),
            style: BoxStyle::default(),
            tag: Some("root".to_string()),
            children: vec![ViewBlueprint::Box {
                rect: Rect::new(100, 50, 120, 40),
                style: BoxStyle::default(),
                tag: Some("child".to_string()),
            }],
        };
        let mut scene = bp.materialize();

        // As spliced into the STATE scene today: verbatim, because nothing
        // lays it out. THIS is the contract ReplaceView relies on.
        let child_rect = |s: &Scene| match s {
            Scene::Container(c) => match &c.children[0] {
                Scene::Box(b) => b.rect,
                _ => panic!("child is a Box"),
            },
            _ => panic!("root is a Container"),
        };
        assert_eq!(
            child_rect(&scene),
            Rect::new(100, 50, 120, 40),
            "unlaid-out (the state-scene path): the wire rect is authoritative",
        );

        // And what a layout pass would do to it — the hazard, pinned.
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 400, 200);
        assert_ne!(
            child_rect(&scene),
            Rect::new(100, 50, 120, 40),
            "a laid-out blueprint LOSES its wire geometry (LayoutStyle is \
             default, so taffy block-flows it). If this assertion ever fails \
             because the rects survived, layout stopped overwriting rect and \
             this whole note can go. If it fails because a blueprint is now \
             spliced somewhere laid out, the wire form must carry layout intent.",
        );
    }
}
