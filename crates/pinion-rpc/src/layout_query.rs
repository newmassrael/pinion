//! `scene/layout` RPC method dispatch (§5.12 R30 — R47.7.1 implement).
//!
//! AI-first paint-introspect primitive: an AI agent asks the framework
//! "what does the paint scene look like at viewport (w, h)?" and gets a
//! typed [`LayoutNode`] tree back, with every node's measured `rect`
//! and the text content where present. No pixels — that channel is
//! `scene/screenshot` (§5.12 R16, still placeholder); this method is
//! the symbolic / tree-shape introspect.
//!
//! Why optional viewport (`dry_run` paint side mirror): the AI sweeps
//! several viewports without changing the application's actual window
//! size. Each request supplies a hypothetical viewport, the handler
//! invokes the application-supplied `paint_producer` closure with
//! those dimensions, runs `compute_layout` (implicit inside the
//! closure), and walks the resulting Scene. The state scene stays
//! untouched — paint scene is a derived view per the §5.7 / §6.3
//! purity contract.
//!
//! Path discipline: the result tree's `path` field is an index-based
//! address `"/0/1/0"` (root → first container child → its 0th child
//! …). Index addressing is sufficient for diagnosis; tag-keyed paths
//! (e.g. `/main_btn/label`) are R47.7.x carry once the application
//! surface (`trait Application`) is ratified.

use pinion_core::Scene;
use serde::{Deserialize, Serialize};

use crate::path::{self, PathError};

/// Request params for `scene/layout`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayoutQueryParams {
    /// R47.7.5 — viewport is now optional. `Some(viewport)` triggers
    /// the hypothetical path (`paint_producer` closure invoked with
    /// the requested dimensions, `dry_run` semantics). `None` returns
    /// the last winit-actually-rendered frame's `LayoutNode` cache —
    /// the application's `last_paint_layout` snapshot. Use `None`
    /// after a `scene/resize` + tick to observe the actual winit frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ViewportSize>,
    /// Optional sub-tree filter. `None` returns the whole paint scene
    /// rooted at the top-level node. R47.7.x carry — currently the
    /// full tree is always returned and the field is accepted for
    /// forward compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Hypothetical viewport size (pixels).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewportSize {
    pub width: u32,
    pub height: u32,
}

/// Recursive paint-scene tree dump (R47.7.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutNode {
    /// Index-based address from the response root (e.g. `"/0/1/0"`).
    pub path: String,
    /// Scene primitive discriminator.
    pub kind: LayoutKind,
    /// Measured rect after `compute_layout` (R24 §5.21 + R47.4 §5.36).
    pub rect: LayoutRect,
    /// Scene node's `tag` field, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Text content. `Some(_)` iff `kind == Text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Number of *visual* lines (UAX #14 break opportunities the
    /// shaper actually broke at) the content produced against
    /// `rect.w`. R51.1 §5.12 — wire-projection of
    /// `pinion_core::scene::TextNode.line_count` (populated by
    /// `pinion-runtime::compute_layout`).
    ///
    /// **Semantic** (UAX #14 visual-line counting; locked so the
    /// parley → §5.37.7 backend swap is observationally stable):
    /// * Soft line breaks (induced by `rect.w`) + hard line breaks
    ///   (`U+000A`) both count. Logical paragraph count is *not*
    ///   exposed by this field.
    /// * BIDI mixed runs (UBA §5.37.4) sitting between the same pair
    ///   of break opportunities occupy one visual line — bidirectional
    ///   content does not inflate the count.
    /// * `content.is_empty()` → `1` (UAX #14 zero-width single line).
    /// * `0` is a sentinel for "no shape pass yet" — Text leaves
    ///   whose owning Scene has not been laid out, and all non-Text
    ///   variants (Container / Box / Path / Image / External /
    ///   Effect / Unknown).
    ///
    /// Lets AI clients verify single-line button labels without
    /// screenshot inspection (Scene-as-data invariant §2 #7).
    pub line_count: u32,
    /// R1641.4 §5.12 — the measured advance INCLUDING trailing whitespace,
    /// where [`Self::rect`]'s width excludes it.
    ///
    /// Wire-projection of `pinion_core::scene::TextNode.advance_px`.
    /// `advance_px - rect.w` is the trailing space the box declined to count,
    /// and a client reading a row of labels that render flush can see the
    /// cause here instead of inferring it from a screenshot (§2 #7).
    ///
    /// `0` for every non-Text variant and for a Text leaf whose owning Scene
    /// has not been laid out — but it carries no sentinel of its own, because
    /// an empty string genuinely advances `0`. [`Self::line_count`] is the
    /// field that distinguishes "not measured" (`0`) from "measured empty"
    /// (`1`).
    pub advance_px: u32,
    /// Recursive children. Empty for leaves.
    pub children: Vec<LayoutNode>,
}

/// Scene primitive discriminator (serializes to lowercase string).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    Container,
    Box,
    Text,
    Path,
    Image,
    External,
    Effect,
    /// Forward-compatibility catch-all for `Scene` variants this
    /// dispatcher predates.
    Unknown,
}

/// Measured rect from `compute_layout` (R24 §5.21).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayoutRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Reasons `scene/layout` can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutQueryError {
    /// The window-prefix portion of `path` failed to parse.
    Path(PathError),
    /// The dispatcher was invoked without a `paint_producer` closure
    /// for a hypothetical-viewport request.
    PaintProducerUnavailable,
    /// `viewport.width` or `viewport.height` is zero — `compute_layout`
    /// requires non-zero extents.
    InvalidViewport,
    /// R47.7.5 — `viewport=None` was supplied but the application has
    /// not yet produced a `last_paint_layout` snapshot (winit has not
    /// rendered a frame yet, or the application skipped registering
    /// the snapshot).
    NoLastPaintLayout,
}

impl From<PathError> for LayoutQueryError {
    fn from(err: PathError) -> Self {
        Self::Path(err)
    }
}

/// Build the [`LayoutNode`] tree for either the hypothetical
/// (`viewport: Some`) or actual winit-rendered (`viewport: None`)
/// path.
///
/// * `Some(viewport)` — invoke `paint_producer` with the requested
///   dimensions and walk the resulting `Scene` (`dry_run` semantics).
/// * `None` — project the addressed window's stored paint scene
///   (R890.1: the same `DispatchContext::last_paint_scene` borrow
///   `scene/snapshot from: paint` serializes — one channel, so the
///   layout READ describes the same frame as the pixel introspection;
///   the projection runs only on this arm, never eagerly).
///
/// # Errors
///
/// See [`LayoutQueryError`] for the full failure surface.
pub fn layout_query<F>(
    params: &LayoutQueryParams,
    paint_producer: Option<&mut F>,
    last_paint_scene: Option<&Scene>,
) -> Result<LayoutNode, LayoutQueryError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    if let Some(p) = &params.path {
        // Validate the window-prefix portion now so caller errors
        // surface uniformly. The remainder is R47.7.x carry — we drop
        // it for now and return the full tree.
        let _ = path::resolve(p)?;
    }
    match params.viewport {
        Some(viewport) => {
            if viewport.width == 0 || viewport.height == 0 {
                return Err(LayoutQueryError::InvalidViewport);
            }
            let producer = paint_producer.ok_or(LayoutQueryError::PaintProducerUnavailable)?;
            let scene = producer(viewport.width, viewport.height);
            Ok(project_layout(&scene))
        }
        None => last_paint_scene
            .map(project_layout)
            .ok_or(LayoutQueryError::NoLastPaintLayout),
    }
}

/// R890 §5.12 — the ONE home of the canonical layout projection: a
/// whole `Scene` becomes a [`LayoutNode`] tree rooted at the `"/0"`
/// wire path. Every `scene/layout` answer goes through here — the
/// viewport-supplied arm above, the GUI substrate's per-window
/// stored-scene projection
/// (`pinion-shell::ShellCore::last_paint_layout_for_window`), and the
/// TUI ingress — so the root prefix cannot drift between backends or
/// read forms (the retired TUI mirror built with a bare `""` prefix,
/// a §2 #6 wire divergence).
#[must_use]
pub fn project_layout(scene: &Scene) -> LayoutNode {
    build_layout_node(scene, "/0")
}

/// Recursive walk: turn a `Scene` sub-tree into a [`LayoutNode`].
/// `path_prefix` is the address of `scene` within the response root
/// (`"/0"` for top-level, `"/0/1"` for the second child of root, etc.).
///
/// Prefer [`project_layout`] for whole-scene projections (it pins the
/// canonical `"/0"` root); this recursive primitive remains public
/// for sub-tree walks and tests.
#[must_use]
pub fn build_layout_node(scene: &Scene, path_prefix: &str) -> LayoutNode {
    let projected = describe_scene(scene);
    let children = projected
        .children
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let child_path = format!("{path_prefix}/{i}");
            build_layout_node(child, &child_path)
        })
        .collect();
    LayoutNode {
        path: path_prefix.to_string(),
        kind: projected.kind,
        rect: projected.rect,
        tag: projected.tag,
        content: projected.content,
        line_count: projected.line_count,
        advance_px: projected.advance_px,
        children,
    }
}

/// Projected view of a `Scene` node into the response shape. R51.1
/// — promoted from an anonymous tuple to a named struct after the
/// `line_count` field broadened the projection beyond what is
/// idiomatic to pattern-match on. The `children` slice is non-empty
/// only for `Scene::Container`; other variants are leaves.
struct SceneProjection<'a> {
    kind: LayoutKind,
    rect: LayoutRect,
    tag: Option<String>,
    content: Option<String>,
    line_count: u32,
    advance_px: u32,
    children: &'a [Scene],
}

fn describe_scene(scene: &Scene) -> SceneProjection<'_> {
    match scene {
        Scene::Container(c) => SceneProjection {
            kind: LayoutKind::Container,
            rect: to_layout_rect(c.rect),
            tag: c.tag.as_ref().map(ToString::to_string),
            content: None,
            line_count: 0,
            advance_px: 0,
            children: c.children.as_slice(),
        },
        Scene::Box(b) => SceneProjection {
            kind: LayoutKind::Box,
            rect: to_layout_rect(b.rect),
            tag: b.tag.as_ref().map(ToString::to_string),
            content: None,
            line_count: 0,
            advance_px: 0,
            children: &[],
        },
        Scene::Text(t) => SceneProjection {
            kind: LayoutKind::Text,
            rect: to_layout_rect(t.rect),
            tag: t.tag.as_ref().map(ToString::to_string),
            content: Some(t.content.clone()),
            line_count: t.line_count,
            advance_px: t.advance_px,
            children: &[],
        },
        Scene::Path(p) => SceneProjection {
            kind: LayoutKind::Path,
            rect: to_layout_rect(p.rect),
            tag: p.tag.as_ref().map(ToString::to_string),
            content: None,
            line_count: 0,
            advance_px: 0,
            children: &[],
        },
        Scene::Image(i) => SceneProjection {
            kind: LayoutKind::Image,
            rect: to_layout_rect(i.rect),
            tag: i.tag.as_ref().map(ToString::to_string),
            content: None,
            line_count: 0,
            advance_px: 0,
            children: &[],
        },
        Scene::External(e) => SceneProjection {
            kind: LayoutKind::External,
            rect: to_layout_rect(e.rect),
            tag: e.tag.as_ref().map(ToString::to_string),
            content: None,
            line_count: 0,
            advance_px: 0,
            children: &[],
        },
        Scene::Effect(_) => SceneProjection {
            kind: LayoutKind::Effect,
            rect: LayoutRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            tag: None,
            content: None,
            line_count: 0,
            advance_px: 0,
            children: &[],
        },
        // Forward-compatibility for future `#[non_exhaustive]` Scene
        // additions — report as Unknown rather than panicking so the
        // AI client can detect newer-than-known variants explicitly.
        _ => SceneProjection {
            kind: LayoutKind::Unknown,
            rect: LayoutRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            tag: None,
            content: None,
            line_count: 0,
            advance_px: 0,
            children: &[],
        },
    }
}

fn to_layout_rect(rect: pinion_core::scene::Rect) -> LayoutRect {
    LayoutRect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
    use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};

    fn make_params(w: u32, h: u32) -> LayoutQueryParams {
        LayoutQueryParams {
            viewport: Some(ViewportSize {
                width: w,
                height: h,
            }),
            path: None,
        }
    }

    #[test]
    fn layout_query_requires_paint_producer_for_hypothetical_path() {
        let params = make_params(320, 200);
        let err = layout_query::<dyn FnMut(u32, u32) -> Scene>(&params, None, None).unwrap_err();
        assert_eq!(err, LayoutQueryError::PaintProducerUnavailable);
    }

    #[test]
    fn layout_query_rejects_zero_viewport() {
        let params = LayoutQueryParams {
            viewport: Some(ViewportSize {
                width: 0,
                height: 200,
            }),
            path: None,
        };
        let mut producer =
            |_w: u32, _h: u32| -> Scene { Scene::Container(ContainerNode::new(vec![])) };
        let err = layout_query(&params, Some(&mut producer), None).unwrap_err();
        assert_eq!(err, LayoutQueryError::InvalidViewport);
    }

    #[test]
    fn layout_query_returns_root_container_with_indexed_path() {
        let params = make_params(320, 200);
        let mut producer = |_w: u32, _h: u32| -> Scene {
            Scene::Container(
                ContainerNode::new(vec![Scene::Box(
                    BoxNode::filled(Rect::new(10, 20, 30, 40), Color::rgb(0xff, 0, 0))
                        .with_tag("button"),
                )])
                .with_style(BoxStyle::filled(Color::rgb(0, 0, 0))),
            )
        };
        let node = layout_query(&params, Some(&mut producer), None).unwrap();
        assert_eq!(node.path, "/0");
        assert_eq!(node.kind, LayoutKind::Container);
        assert_eq!(node.children.len(), 1);
        let child = &node.children[0];
        assert_eq!(child.path, "/0/0");
        assert_eq!(child.kind, LayoutKind::Box);
        assert_eq!(child.tag.as_deref(), Some("button"));
        assert_eq!(
            child.rect,
            LayoutRect {
                x: 10,
                y: 20,
                w: 30,
                h: 40
            }
        );
    }

    #[test]
    fn layout_query_text_node_exposes_content_and_rect() {
        // R47.7.1 — wrap diagnosis primitive: AI inspects text rect.h
        // to detect single/multi-line wrap. content surfaces so the
        // tree dump is self-describing without re-querying state.
        // R51.1 — `line_count` is a Text-only sidecar populated by
        // `compute_layout`. This test builds a raw `Scene::Text` and
        // does not invoke layout, so the default `line_count = 0`
        // round-trips through the projection unchanged.
        let params = make_params(320, 200);
        let mut producer = |_w: u32, _h: u32| -> Scene {
            Scene::Container(ContainerNode::new(vec![Scene::Text(
                TextNode::styled(
                    "Click me!",
                    Rect::new(50, 30, 60, 22),
                    TextStyle::new().with_size_px(18),
                )
                .with_tag("label"),
            )]))
        };
        let node = layout_query(&params, Some(&mut producer), None).unwrap();
        let text = &node.children[0];
        assert_eq!(text.kind, LayoutKind::Text);
        assert_eq!(text.content.as_deref(), Some("Click me!"));
        assert_eq!(
            text.rect,
            LayoutRect {
                x: 50,
                y: 30,
                w: 60,
                h: 22
            }
        );
        assert_eq!(text.tag.as_deref(), Some("label"));
        assert_eq!(text.line_count, 0, "raw Scene::Text default");
    }

    #[test]
    fn layout_query_text_line_count_round_trips_through_projection() {
        // R51.1 §5.12 — TextNode.line_count (the measured-result
        // sidecar populated by `pinion-runtime::compute_layout`)
        // projects unchanged through `build_layout_node` onto
        // `LayoutNode.line_count`. The runtime-side measurement is
        // covered by `pinion-runtime::layout::tests::text_*`; this
        // test owns only the wire-projection direction.
        let mut measured_text = TextNode::styled(
            "Click me!",
            Rect::new(50, 30, 60, 22),
            TextStyle::new().with_size_px(18),
        )
        .with_tag("label");
        measured_text.line_count = 1;
        let params = make_params(320, 200);
        let mut producer = |_w: u32, _h: u32| -> Scene {
            Scene::Container(ContainerNode::new(vec![Scene::Text(measured_text.clone())]))
        };
        let node = layout_query(&params, Some(&mut producer), None).unwrap();
        let text = &node.children[0];
        assert_eq!(text.line_count, 1);
    }

    #[test]
    fn layout_query_non_text_node_line_count_is_zero() {
        // R51.1 — `line_count` is Text-specific; Container/Box/Path/
        // Image/External/Effect/Unknown all project as 0. Locks the
        // semantics so AI clients can predicate on `kind == Text &&
        // line_count > 1` without false positives from non-Text
        // variants.
        let params = make_params(320, 200);
        let mut producer = |_w: u32, _h: u32| -> Scene {
            Scene::Container(
                ContainerNode::new(vec![Scene::Box(
                    BoxNode::filled(Rect::new(10, 20, 30, 40), Color::rgb(0xff, 0, 0))
                        .with_tag("button"),
                )])
                .with_style(BoxStyle::filled(Color::rgb(0, 0, 0))),
            )
        };
        let root = layout_query(&params, Some(&mut producer), None).unwrap();
        assert_eq!(root.line_count, 0, "Container line_count");
        assert_eq!(root.children[0].line_count, 0, "Box line_count");
    }

    #[test]
    fn layout_query_invokes_producer_with_requested_viewport() {
        // The producer must receive exactly the viewport from
        // `params.viewport` — the AI sweeps viewports to detect
        // resize-triggered wrap variation.
        let params = make_params(345, 200);
        let captured = std::cell::Cell::new((0_u32, 0_u32));
        let mut producer = |w: u32, h: u32| -> Scene {
            captured.set((w, h));
            Scene::Container(
                ContainerNode::new(vec![])
                    .with_layout(LayoutStyle::new().with_size(Size::px(w, h))),
            )
        };
        let _ = layout_query(&params, Some(&mut producer), None).unwrap();
        assert_eq!(captured.get(), (345, 200));
    }

    #[test]
    fn layout_query_viewport_none_projects_the_stored_paint_scene() {
        // R47.7.5 / R890.1 — viewport=None projects the stored paint
        // scene (the same borrow `scene/snapshot from: paint` reads;
        // winit-actual frame). The paint_producer closure is not
        // invoked — viewport=None is the actual-frame path, not the
        // hypothetical path.
        let params = LayoutQueryParams {
            viewport: None,
            path: None,
        };
        let mut stored_container = ContainerNode::new(vec![]);
        stored_container.rect = pinion_core::scene::Rect::new(0, 0, 320, 200);
        let stored = Scene::Container(stored_container);
        let mut producer_called = false;
        let mut producer = |_w: u32, _h: u32| -> Scene {
            producer_called = true;
            Scene::Container(ContainerNode::new(vec![]))
        };
        let node = layout_query(&params, Some(&mut producer), Some(&stored)).unwrap();
        assert_eq!(node.path, "/0", "canonical projection root");
        assert_eq!(
            (node.rect.w, node.rect.h),
            (320, 200),
            "stored frame's geometry"
        );
        assert!(!producer_called);
    }

    #[test]
    fn layout_query_viewport_none_without_cache_errors() {
        // R47.7.5 — when viewport=None is requested before winit has
        // rendered a frame (or before the application registered the
        // cache surface), surface NoLastPaintLayout so the AI client
        // can retry after `scene/resize` + tick.
        let params = LayoutQueryParams {
            viewport: None,
            path: None,
        };
        let err = layout_query::<dyn FnMut(u32, u32) -> Scene>(&params, None, None).unwrap_err();
        assert_eq!(err, LayoutQueryError::NoLastPaintLayout);
    }
}
