//! `scene/snapshot` RPC method dispatch (§5.12 method 4 of 7, R16 slice 13).
//!
//! Captures the scene tree shape and, for `Scene::External` that opts
//! in to §5.15 item 8 introspection, dumps every `(path, value)` pair
//! declared by `ExternalIntrospect::schema`.
//!
//! R51.194 §5.49 §5.45 — the dispatcher now recurses into
//! `Scene::Container.children` and `Scene::Scroll.content`, exposing
//! the container tag list and the scroll viewport / offset / tag so
//! AI-side dogfood demos (see `tools/demos/`) can enumerate visible
//! widgets without screenshot prose. Leaf primitives (`Box`, `Text`,
//! `Path`, `Image`, `Effect`) stay marker-only — tag / content
//! exposure for those is a R51.197+ carry once a demo needs them.
//!
//! Surface details:
//!   * path: `/[window[id]/]` only — no scene-path tail, since v0 has
//!     no addressable sub-tree shape (`scene/query` is the typed
//!     descend-by-path channel; snapshot is the whole-tree dump).
//!   * leaf primitives (`Box`, `Text`, `Path`, `Image`, `Effect`)
//!     report only their discriminator.
//!   * `Container` exposes its `tag` and recurses through `children`.
//!   * `Scroll` exposes `tag`, `viewport` rect, `(offset_x, offset_y)`,
//!     and recurses through `content` — the §5.45 R55 substrate fields
//!     a Scroll-aware demo needs to assert the visible row window.
//!   * `External` dumps its `ExternalIntrospect::schema` fields.
//!   * fallback [`SnapshotNode::Unknown`] keeps the dispatcher
//!     forward-compatible with `non_exhaustive` `Scene` additions in
//!     pinion-core.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::Rect;
use pinion_core::Scene;

use crate::path::{self, PathError};

/// One scene tree primitive's snapshot shape.
///
/// R51.194 §5.49 §5.45 — `Container` and `Scroll` are tuple variants
/// carrying their tag plus a recursive child snapshot, so a single
/// `scene/snapshot` call dumps the whole tree the AI client needs to
/// reason about. Leaf primitives stay unit-variant markers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotNode {
    Box,
    Text,
    Path,
    Image,
    Container(ContainerSnapshot),
    Effect,
    External(ExternalSnapshot),
    /// R51.194 §5.45 — `Scene::Scroll` snapshot carrying the §5.45 R55
    /// substrate fields the harness uses to assert visible window /
    /// scroll position from a demo.
    Scroll(ScrollSnapshot),
    /// Catch-all for `Scene` variants added in a later pinion-core
    /// version that this dispatcher predates.
    Unknown,
}

/// `External` payload of [`SnapshotNode::External`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSnapshot {
    /// `Some(fields)` when the `External` opted in to §5.15 item 8 and
    /// reported a schema; `None` otherwise. Order matches the schema's
    /// declared field order.
    pub introspect: Option<Vec<(String, IntrospectValue)>>,
}

/// `Container` payload of [`SnapshotNode::Container`] (R51.194 §5.49).
///
/// `tag` mirrors `ContainerNode.tag` (the §5.20 intent-routing handle).
/// `children` is a depth-first traversal of `ContainerNode.children`;
/// each entry is itself a `SnapshotNode`, including nested containers
/// and scrolls, so a single root snapshot is the whole tree.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerSnapshot {
    pub tag: Option<String>,
    pub children: Vec<SnapshotNode>,
}

/// `Scroll` payload of [`SnapshotNode::Scroll`] (R51.194 §5.49 §5.45).
///
/// Exposes the §5.45 R55 substrate fields a Scroll demo needs:
///   * `tag` mirrors `ScrollNode.tag` (input-router handle, e.g.
///     `"main_listbox"`),
///   * `viewport` is the clip rect in logical pixels / cells,
///   * `offset_x` / `offset_y` are the current scroll position,
///   * `content` is the (recursive) snapshot of the scene clipped by
///     this scroll — typically a `Container` of widget rows.
///
/// The reactive `state` link on `ScrollNode` is *not* exposed — `Rc`
/// handles are not serialisable, and the declarative fields are
/// sufficient for AI-side assertions (offset / viewport / tree
/// shape).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollSnapshot {
    pub tag: Option<String>,
    pub viewport: Rect,
    pub offset_x: i32,
    pub offset_y: i32,
    pub content: Box<SnapshotNode>,
}

/// Reasons [`snapshot`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path is not v0-supported (must be empty after window
    /// prefix).
    UnsupportedPath,
}

impl From<PathError> for SnapshotError {
    fn from(err: PathError) -> Self {
        SnapshotError::Path(err)
    }
}

/// Build a snapshot of `scene` at the resolved window root.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the window prefix is malformed or the
/// scene path carries an unsupported tail.
pub fn snapshot(scene: &Scene, raw_path: &str) -> Result<SnapshotNode, SnapshotError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // v0: scene path must be empty. Tails like /external/count belong
    // to scene/query, not scene/snapshot.
    if !resolved.scene_path.is_empty() {
        return Err(SnapshotError::UnsupportedPath);
    }

    Ok(snapshot_root(scene))
}

fn snapshot_root(scene: &Scene) -> SnapshotNode {
    match scene {
        Scene::Box(_) => SnapshotNode::Box,
        Scene::Text(_) => SnapshotNode::Text,
        Scene::Path(_) => SnapshotNode::Path,
        Scene::Image(_) => SnapshotNode::Image,
        Scene::Container(node) => SnapshotNode::Container(ContainerSnapshot {
            tag: node.tag.as_ref().map(|t| t.as_ref().to_string()),
            children: node.children.iter().map(snapshot_root).collect(),
        }),
        Scene::Effect(_) => SnapshotNode::Effect,
        Scene::External(node) => {
            let introspect = node.handle.introspect().map(|intro| {
                intro
                    .schema()
                    .fields
                    .iter()
                    .filter_map(|(name, _ty)| intro.query(name).map(|v| ((*name).to_string(), v)))
                    .collect()
            });
            SnapshotNode::External(ExternalSnapshot { introspect })
        }
        Scene::Scroll(node) => SnapshotNode::Scroll(ScrollSnapshot {
            tag: node.tag.as_ref().map(|t| t.as_ref().to_string()),
            viewport: node.viewport,
            offset_x: node.offset_x,
            offset_y: node.offset_y,
            content: Box::new(snapshot_root(node.content.as_ref())),
        }),
        // `Scene` is non_exhaustive; future variants surface as Unknown
        // until this dispatcher is updated for them.
        _ => SnapshotNode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};
    use pinion_core::Color;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn box_root_snapshots_as_box() {
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        assert_eq!(snapshot(&scene, "").unwrap(), SnapshotNode::Box);
    }

    #[test]
    fn counted_external_dumps_introspect_fields() {
        let scene = counted_scene(42);
        let snap = snapshot(&scene, "").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
            }) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "count");
                assert_eq!(fields[0].1, IntrospectValue::Int(42));
            }
            other => panic!("expected External with introspect Some, got {other:?}"),
        }
    }

    #[test]
    fn stub_external_introspect_is_none() {
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let snap = snapshot(&scene, "").unwrap();
        assert_eq!(
            snap,
            SnapshotNode::External(ExternalSnapshot { introspect: None }),
        );
    }

    #[test]
    fn window_prefix_short_circuits() {
        let scene = counted_scene(0);
        let snap = snapshot(&scene, "/window[main]").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
            }) => assert_eq!(fields[0].1, IntrospectValue::Int(0)),
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn scene_path_tail_is_unsupported() {
        let scene = counted_scene(0);
        let err = snapshot(&scene, "/external/count").unwrap_err();
        assert_eq!(err, SnapshotError::UnsupportedPath);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = counted_scene(0);
        let err = snapshot(&scene, "/window[main").unwrap_err();
        assert_eq!(err, SnapshotError::Path(PathError::MalformedPrefix));
    }

    mod r51_194 {
        use super::*;
        use pinion_core::scene::{
            ContainerNode, ScrollNode, TextNode,
        };

        fn leaf_box(tag: Option<&'static str>) -> Scene {
            let node = BoxNode::filled(Rect::new(0, 0, 10, 10), Color::default());
            if let Some(t) = tag {
                Scene::Box(node.with_tag(t))
            } else {
                Scene::Box(node)
            }
        }

        #[test]
        fn container_root_dumps_tag_and_children() {
            let scene = Scene::Container(
                ContainerNode::new(vec![leaf_box(None), leaf_box(Some("inner"))])
                    .with_tag("root"),
            );
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Container(snap) => {
                    assert_eq!(snap.tag.as_deref(), Some("root"));
                    assert_eq!(snap.children.len(), 2);
                    assert_eq!(snap.children[0], SnapshotNode::Box);
                    assert_eq!(snap.children[1], SnapshotNode::Box);
                }
                other => panic!("expected Container, got {other:?}"),
            }
        }

        #[test]
        fn untagged_container_reports_none_tag() {
            let scene = Scene::Container(ContainerNode::new(vec![]));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Container(snap) => {
                    assert!(snap.tag.is_none());
                    assert!(snap.children.is_empty());
                }
                other => panic!("expected Container, got {other:?}"),
            }
        }

        #[test]
        fn nested_container_recurses_depth_first() {
            let inner = Scene::Container(ContainerNode::new(vec![leaf_box(None)]).with_tag("inner"));
            let scene = Scene::Container(ContainerNode::new(vec![inner]).with_tag("outer"));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Container(outer) => {
                    assert_eq!(outer.tag.as_deref(), Some("outer"));
                    assert_eq!(outer.children.len(), 1);
                    match &outer.children[0] {
                        SnapshotNode::Container(inner) => {
                            assert_eq!(inner.tag.as_deref(), Some("inner"));
                            assert_eq!(inner.children.len(), 1);
                            assert_eq!(inner.children[0], SnapshotNode::Box);
                        }
                        other => panic!("expected nested Container, got {other:?}"),
                    }
                }
                other => panic!("expected Container, got {other:?}"),
            }
        }

        #[test]
        fn scroll_root_dumps_viewport_offset_tag_content() {
            let content = Scene::Text(TextNode::new("row".to_string(), Rect::new(0, 0, 50, 200)));
            let scroll = ScrollNode::new(Rect::new(0, 0, 50, 80), content)
                .with_tag("listbox_scroll");
            let scene = Scene::Scroll(scroll.with_offset(0, 120));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Scroll(snap) => {
                    assert_eq!(snap.tag.as_deref(), Some("listbox_scroll"));
                    assert_eq!(snap.viewport, Rect::new(0, 0, 50, 80));
                    assert_eq!(snap.offset_x, 0);
                    assert_eq!(snap.offset_y, 120);
                    assert_eq!(*snap.content, SnapshotNode::Text);
                }
                other => panic!("expected Scroll, got {other:?}"),
            }
        }

        #[test]
        fn untagged_scroll_reports_none_tag_and_zero_offset() {
            let content = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
            let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 10, 10), content));
            match snapshot(&scene, "").unwrap() {
                SnapshotNode::Scroll(snap) => {
                    assert!(snap.tag.is_none());
                    assert_eq!(snap.offset_x, 0);
                    assert_eq!(snap.offset_y, 0);
                }
                other => panic!("expected Scroll, got {other:?}"),
            }
        }

        #[test]
        fn scroll_inside_container_traverses_both_layers() {
            let row = leaf_box(Some("row"));
            let scroll = ScrollNode::new(Rect::new(0, 0, 50, 80), Scene::Container(
                ContainerNode::new(vec![row]).with_tag("rows"),
            ))
            .with_tag("scroll");
            let scene = Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(scroll)]).with_tag("root"),
            );
            let SnapshotNode::Container(outer) = snapshot(&scene, "").unwrap() else {
                panic!("expected outer Container");
            };
            assert_eq!(outer.tag.as_deref(), Some("root"));
            let SnapshotNode::Scroll(scroll_snap) = &outer.children[0] else {
                panic!("expected Scroll inside Container");
            };
            assert_eq!(scroll_snap.tag.as_deref(), Some("scroll"));
            let SnapshotNode::Container(rows) = &*scroll_snap.content else {
                panic!("expected Container inside Scroll content");
            };
            assert_eq!(rows.tag.as_deref(), Some("rows"));
            assert_eq!(rows.children.len(), 1);
            assert_eq!(rows.children[0], SnapshotNode::Box);
        }
    }
}
