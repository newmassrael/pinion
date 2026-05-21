//! `scene/snapshot` RPC method dispatch (§5.12 method 4 of 7, R16 slice 13).
//!
//! Captures the scene tree shape and, for `Scene::External` that opts
//! in to §5.15 item 8 introspection, dumps every `(path, value)` pair
//! declared by `ExternalIntrospect::schema`.
//!
//! R51.194 §5.49 §5.45 — the dispatcher recurses into
//! `Scene::Container.children` and `Scene::Scroll.content`.
//!
//! R51.198 §5.49 — leaf primitives (`Box`, `Text`, `Path`, `Image`)
//! and the `Container` / `External` parents now expose `rect` + `tag`
//! (and `content` for `Text`). With this layer the AI-side harness can
//! locate widgets by tag and derive bboxes without hardcoding pixel
//! coordinates per demo (see `tools/demos/hello_toggle_click.py` first
//! consumer). Only `Effect` stays opaque per §3 capability boundary.
//!
//! R55.G.8 §5.49 — `Box`, `Text`, and `Container` additionally surface
//! their `BoxStyle` / `TextStyle` sidecars (fill, border, corner radius,
//! font size / colour / weight / style). The wire JSON converts each
//! style to a structured object — `{r, g, b, a}` for colours, named
//! variants for enums — so AI clients can introspect the rendered look
//! of any widget without inspecting pixels (§2 #7 scene-as-data).
//!
//! Surface details:
//!   * path: `/[window[id]/]` only — no scene-path tail, since v0 has
//!     no addressable sub-tree shape (`scene/query` is the typed
//!     descend-by-path channel; snapshot is the whole-tree dump).
//!   * leaf primitives (`Box`, `Text`, `Path`, `Image`) report
//!     `rect` and optional `tag`. `Text` additionally reports
//!     `content` (§2 #7 scene-as-data invariant).
//!   * `Container` exposes `rect`, `tag`, and recurses through
//!     `children`.
//!   * `Scroll` exposes `tag`, `viewport` rect, `(offset_x, offset_y)`,
//!     and recurses through `content` — the §5.45 R55 substrate fields
//!     a Scroll-aware demo needs to assert the visible row window.
//!     `viewport` IS the scroll primitive's geometry (per
//!     `ScrollNode::viewport` doc) so no separate `rect` field.
//!   * `External` exposes `rect`, `tag`, and the
//!     `ExternalIntrospect::schema` fields per §5.15 item 8.
//!   * `Effect` is opaque per §3 — no exposure beyond the discriminator.
//!   * fallback [`SnapshotNode::Unknown`] keeps the dispatcher
//!     forward-compatible with `non_exhaustive` `Scene` additions in
//!     pinion-core.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::Rect;
use pinion_core::style::{BoxStyle, TextStyle};
use pinion_core::Scene;

use crate::path::{self, PathError};

/// One scene tree primitive's snapshot shape.
///
/// R51.198 §5.49 — leaf primitives carry a payload struct exposing
/// `rect` and optional `tag`. `Effect` stays unit-marker per §3
/// opaque-primitive boundary, and `Unknown` is the forward-compatible
/// catch-all.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotNode {
    Box(BoxSnapshot),
    Text(TextSnapshot),
    Path(PathSnapshot),
    Image(ImageSnapshot),
    Container(ContainerSnapshot),
    Effect,
    External(ExternalSnapshot),
    Scroll(ScrollSnapshot),
    /// Catch-all for `Scene` variants added in a later pinion-core
    /// version that this dispatcher predates.
    Unknown,
}

/// `Box` payload of [`SnapshotNode::Box`] (R51.198 §5.49,
/// R55.G.8 added `style`).
///
/// `rect` mirrors `BoxNode.rect`; `tag` mirrors `BoxNode.tag`;
/// `style` mirrors `BoxNode.style` so AI clients can introspect the
/// rendered fill, border, and corner radius without OCR (§2 #7).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct BoxSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub style: BoxStyle,
}

/// `Text` payload of [`SnapshotNode::Text`] (R51.198 §5.49,
/// R55.G.8 added `style`).
///
/// `content` mirrors `TextNode.content` — pinion exposes text as data
/// per §2 invariant #7 (scene-as-data), so AI clients can read the
/// rendered text without OCR'ing a screenshot. `style` mirrors
/// `TextNode.style` to surface the visual axis (font family / size /
/// colour / weight / style) on the same scene-as-data principle.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct TextSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub content: String,
    pub style: TextStyle,
}

/// `Path` payload of [`SnapshotNode::Path`] (R51.198 §5.49 + carry).
///
/// `commands` mirrors `PathNode.commands` — the structured command
/// stream the rasterizer consumes. Exposing the commands keeps
/// `scene-as-data` (§2 #7) complete for vector geometry: an AI
/// agent can introspect path shape without OCR'ing the painted
/// result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PathSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub commands: Vec<pinion_core::scene::PathCommand>,
}

/// `Image` payload of [`SnapshotNode::Image`] (R51.198 §5.49 + carry).
///
/// `source` mirrors `ImageNode.source` — typically the asset URI or
/// path the application loaded. The string is opaque to the
/// framework but lets an AI agent verify "this is the right icon"
/// without inspecting pixels (§2 #7 scene-as-data).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub source: String,
}

/// `Container` payload of [`SnapshotNode::Container`] (R51.194 §5.49,
/// R51.198 added `rect`, R55.G.8 added `style`).
///
/// `tag` mirrors `ContainerNode.tag` (the §5.20 intent-routing handle).
/// `rect` mirrors `ContainerNode.rect`.
/// `style` mirrors `ContainerNode.style` — Container nodes paint their
/// own fill / border / corner radius before recursing into children,
/// so the same §2 #7 scene-as-data surface applies as for `BoxNode`.
/// `children` is a depth-first traversal of `ContainerNode.children`;
/// each entry is itself a `SnapshotNode`, including nested containers
/// and scrolls, so a single root snapshot is the whole tree.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub style: BoxStyle,
    pub children: Vec<SnapshotNode>,
}

/// `Scroll` payload of [`SnapshotNode::Scroll`] (R51.194 §5.49 §5.45).
///
/// Exposes the §5.45 R55 substrate fields a Scroll demo needs:
///   * `tag` mirrors `ScrollNode.tag` (input-router handle, e.g.
///     `"main_listbox"`),
///   * `viewport` is the clip rect in logical pixels / cells — also
///     the scroll primitive's geometry (no separate `rect` field),
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

/// `External` payload of [`SnapshotNode::External`] (R51.198 added
/// `rect` + `tag`).
///
/// `rect` mirrors `ExternalNode.rect`; `tag` mirrors `ExternalNode.tag`
/// (the §5.20 intent-routing handle used by widgets like
/// `main_toggle`). `introspect` is `Some(fields)` when the `External`
/// opted in to §5.15 item 8 and reported a schema; `None` otherwise.
/// Order matches the schema's declared field order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSnapshot {
    pub rect: Rect,
    pub tag: Option<String>,
    pub introspect: Option<Vec<(String, IntrospectValue)>>,
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

fn cow_to_owned(tag: Option<&std::borrow::Cow<'static, str>>) -> Option<String> {
    tag.map(|t| t.as_ref().to_string())
}

fn snapshot_root(scene: &Scene) -> SnapshotNode {
    match scene {
        Scene::Box(node) => SnapshotNode::Box(BoxSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            style: node.style,
        }),
        Scene::Text(node) => SnapshotNode::Text(TextSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            content: node.content.clone(),
            style: node.style.clone(),
        }),
        Scene::Path(node) => SnapshotNode::Path(PathSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            commands: node.commands.clone(),
        }),
        Scene::Image(node) => SnapshotNode::Image(ImageSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            source: node.source.clone(),
        }),
        Scene::Container(node) => SnapshotNode::Container(ContainerSnapshot {
            rect: node.rect,
            tag: cow_to_owned(node.tag.as_ref()),
            style: node.style,
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
            SnapshotNode::External(ExternalSnapshot {
                rect: node.rect,
                tag: cow_to_owned(node.tag.as_ref()),
                introspect,
            })
        }
        Scene::Scroll(node) => SnapshotNode::Scroll(ScrollSnapshot {
            tag: cow_to_owned(node.tag.as_ref()),
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
        match snapshot(&scene, "").unwrap() {
            SnapshotNode::Box(snap) => {
                assert_eq!(snap.rect, Rect::default());
                assert!(snap.tag.is_none());
            }
            other => panic!("expected Box, got {other:?}"),
        }
    }

    #[test]
    fn counted_external_dumps_introspect_fields() {
        let scene = counted_scene(42);
        let snap = snapshot(&scene, "").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
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
        match snapshot(&scene, "").unwrap() {
            SnapshotNode::External(snap) => {
                assert!(snap.introspect.is_none());
            }
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn window_prefix_short_circuits() {
        let scene = counted_scene(0);
        let snap = snapshot(&scene, "/window[main]").unwrap();
        match snap {
            SnapshotNode::External(ExternalSnapshot {
                introspect: Some(fields),
                ..
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
        use pinion_core::scene::{ContainerNode, ScrollNode, TextNode};

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
                    matches_box(&snap.children[0], None);
                    matches_box(&snap.children[1], Some("inner"));
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
                            matches_box(&inner.children[0], None);
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
                    matches_text(&snap.content, "row", Rect::new(0, 0, 50, 200));
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
            let scroll = ScrollNode::new(
                Rect::new(0, 0, 50, 80),
                Scene::Container(ContainerNode::new(vec![row]).with_tag("rows")),
            )
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
            matches_box(&rows.children[0], Some("row"));
        }

        fn matches_box(node: &SnapshotNode, tag: Option<&str>) {
            match node {
                SnapshotNode::Box(snap) => {
                    assert_eq!(snap.tag.as_deref(), tag);
                }
                other => panic!("expected Box, got {other:?}"),
            }
        }

        fn matches_text(node: &SnapshotNode, content: &str, rect: Rect) {
            match node {
                SnapshotNode::Text(snap) => {
                    assert_eq!(snap.content, content);
                    assert_eq!(snap.rect, rect);
                }
                other => panic!("expected Text, got {other:?}"),
            }
        }
    }

    mod r51_198 {
        //! Leaf primitive `rect` + `tag` exposure tests.
        //!
        //! Each Scene primitive surfaces its geometry and intent-routing
        //! tag through the new `SnapshotNode` payload structs, so demos
        //! can locate widgets by tag and derive click / scroll
        //! coordinates from the snapshot instead of hardcoding pixels.

        use super::*;
        use pinion_core::scene::{
            ContainerNode, ImageNode, PathCommand, PathNode, PathPoint, TextNode,
        };
        use pinion_core::style::PathStyle;

        #[test]
        fn box_carries_rect_and_tag() {
            let node = BoxNode::filled(Rect::new(10, 20, 30, 40), Color::default())
                .with_tag("box_tag");
            let scene = Scene::Box(node);
            let SnapshotNode::Box(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Box");
            };
            assert_eq!(snap.rect, Rect::new(10, 20, 30, 40));
            assert_eq!(snap.tag.as_deref(), Some("box_tag"));
        }

        #[test]
        fn text_carries_rect_tag_and_content() {
            let node = TextNode::new("hello".to_string(), Rect::new(5, 6, 50, 14))
                .with_tag("greeting");
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert_eq!(snap.rect, Rect::new(5, 6, 50, 14));
            assert_eq!(snap.tag.as_deref(), Some("greeting"));
            assert_eq!(snap.content, "hello");
        }

        #[test]
        fn untagged_text_reports_none_tag() {
            let node = TextNode::new(String::new(), Rect::default());
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert!(snap.tag.is_none());
            assert_eq!(snap.content, "");
        }

        #[test]
        fn path_carries_rect_tag_and_commands() {
            let commands = vec![
                PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
                PathCommand::LineTo(PathPoint::new(50.0, 0.0)),
                PathCommand::Close,
            ];
            let node = PathNode::new(
                Rect::new(0, 0, 100, 100),
                commands.clone(),
                PathStyle::default(),
            )
            .with_tag("chevron");
            let scene = Scene::Path(node);
            let SnapshotNode::Path(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Path");
            };
            assert_eq!(snap.rect, Rect::new(0, 0, 100, 100));
            assert_eq!(snap.tag.as_deref(), Some("chevron"));
            assert_eq!(snap.commands, commands);
        }

        #[test]
        fn image_carries_rect_tag_and_source() {
            let node = ImageNode::new("icon.png", Rect::new(8, 8, 16, 16))
                .with_tag("logo");
            let scene = Scene::Image(node);
            let SnapshotNode::Image(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Image");
            };
            assert_eq!(snap.rect, Rect::new(8, 8, 16, 16));
            assert_eq!(snap.tag.as_deref(), Some("logo"));
            assert_eq!(snap.source, "icon.png");
        }

        #[test]
        fn container_carries_rect_alongside_tag_and_children() {
            // `ContainerNode.rect` is layout-derived in production, so
            // the test mutates the field directly after the builder
            // chain — the snapshot pipeline reads whichever value the
            // node currently carries.
            let mut node = ContainerNode::new(vec![]).with_tag("hello_toggle_root");
            node.rect = Rect::new(0, 0, 360, 220);
            let scene = Scene::Container(node);
            let SnapshotNode::Container(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Container");
            };
            assert_eq!(snap.rect, Rect::new(0, 0, 360, 220));
            assert_eq!(snap.tag.as_deref(), Some("hello_toggle_root"));
            assert!(snap.children.is_empty());
        }

        #[test]
        fn external_carries_rect_and_tag_alongside_introspect() {
            let mut node =
                ExternalNode::new(Box::new(CountedExternal::new(5))).with_tag("main_toggle");
            node.rect = Rect::new(100, 50, 64, 32);
            let scene = Scene::External(node);
            let SnapshotNode::External(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected External");
            };
            assert_eq!(snap.rect, Rect::new(100, 50, 64, 32));
            assert_eq!(snap.tag.as_deref(), Some("main_toggle"));
            let fields = snap.introspect.expect("CountedExternal opts into introspect");
            assert_eq!(fields[0].0, "count");
            assert_eq!(fields[0].1, IntrospectValue::Int(5));
        }
    }

    mod r55_g8 {
        //! R55.G.8 §5.49 — `BoxStyle` + `TextStyle` snapshot exposure.
        //!
        //! Box / Text / Container now surface their style sidecars
        //! (fill, border, corner radius, font axis) so AI clients can
        //! verify rendered chrome and typography without OCR.

        use super::*;
        use pinion_core::scene::{ContainerNode, TextNode};
        use pinion_core::style::{
            Border, BorderPlacement, BoxStyle, FontStyle, FontWeight, TextStyle,
        };

        #[test]
        fn box_carries_full_style_with_border_and_corner_radius() {
            let style = BoxStyle::filled(Color::rgba(0x11, 0x22, 0x33, 0xff))
                .with_border(
                    Border::new(Color::rgba(0xaa, 0xbb, 0xcc, 0xff), 3)
                        .with_placement(BorderPlacement::Outside),
                )
                .with_corner_radius(8);
            let node = BoxNode::filled(Rect::new(0, 0, 32, 32), Color::default());
            // BoxNode::filled sets the fill via the constructor; rewrite
            // the whole style sidecar to attach border + corner_radius
            // without going through every with_* builder permutation.
            let mut node = node;
            node.style = style;
            let scene = Scene::Box(node);
            let SnapshotNode::Box(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Box");
            };
            assert_eq!(snap.style.fill, Color::rgba(0x11, 0x22, 0x33, 0xff));
            let border = snap.style.border.expect("border preserved");
            assert_eq!(border.color, Color::rgba(0xaa, 0xbb, 0xcc, 0xff));
            assert_eq!(border.width, 3);
            assert_eq!(border.placement, BorderPlacement::Outside);
            assert_eq!(snap.style.corner_radius, 8);
        }

        #[test]
        fn text_carries_full_visual_style() {
            let style = TextStyle::new()
                .with_size_px(20)
                .with_fg(Color::rgba(0x55, 0x66, 0x77, 0xff))
                .with_weight(FontWeight::BOLD)
                .with_style(FontStyle::Italic);
            let mut node = TextNode::new("hi".to_string(), Rect::new(0, 0, 40, 20));
            node.style = style;
            let scene = Scene::Text(node);
            let SnapshotNode::Text(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Text");
            };
            assert_eq!(snap.style.font_size_px, 20);
            assert_eq!(snap.style.fg_color, Color::rgba(0x55, 0x66, 0x77, 0xff));
            assert_eq!(snap.style.font_weight, FontWeight::BOLD);
            assert_eq!(snap.style.font_style, FontStyle::Italic);
        }

        #[test]
        fn container_carries_style_alongside_children() {
            let mut node = ContainerNode::new(vec![]).with_tag("frame");
            node.style = BoxStyle::filled(Color::rgba(0x12, 0x34, 0x56, 0x78))
                .with_corner_radius(4);
            let scene = Scene::Container(node);
            let SnapshotNode::Container(snap) = snapshot(&scene, "").unwrap() else {
                panic!("expected Container");
            };
            assert_eq!(snap.style.fill, Color::rgba(0x12, 0x34, 0x56, 0x78));
            assert_eq!(snap.style.corner_radius, 4);
            assert!(snap.style.border.is_none());
        }
    }
}
