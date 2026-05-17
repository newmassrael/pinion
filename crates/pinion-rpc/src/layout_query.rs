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
    /// Hypothetical viewport dimensions. The handler invokes the
    /// `paint_producer` closure with `(viewport.width, viewport.height)`
    /// and `compute_layout` runs inside that closure.
    pub viewport: ViewportSize,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutQueryError {
    /// The window-prefix portion of `path` failed to parse.
    Path(PathError),
    /// The dispatcher was invoked without a `paint_producer` closure.
    /// `DispatchContext::with_paint_producer` is the application-side
    /// surface that registers one.
    PaintProducerUnavailable,
    /// `viewport.width` or `viewport.height` is zero — `compute_layout`
    /// requires non-zero extents.
    InvalidViewport,
}

impl From<PathError> for LayoutQueryError {
    fn from(err: PathError) -> Self {
        Self::Path(err)
    }
}

/// Build the [`LayoutNode`] tree by invoking `paint_producer` with the
/// requested viewport and walking the resulting [`Scene`].
///
/// Returns [`LayoutQueryError::PaintProducerUnavailable`] when the
/// caller did not register a closure; this is a non-recoverable shape
/// mismatch reported as `InvalidParams (-32602)` upstream.
///
/// # Errors
///
/// See [`LayoutQueryError`] for the full failure surface.
pub fn layout_query<F>(
    params: &LayoutQueryParams,
    paint_producer: Option<&mut F>,
) -> Result<LayoutNode, LayoutQueryError>
where
    F: FnMut(u32, u32) -> Scene + ?Sized,
{
    if params.viewport.width == 0 || params.viewport.height == 0 {
        return Err(LayoutQueryError::InvalidViewport);
    }
    if let Some(p) = &params.path {
        // Validate the window-prefix portion now so caller errors
        // surface uniformly. The remainder is R47.7.x carry — we drop
        // it for now and return the full tree.
        let _ = path::resolve(p)?;
    }
    let producer = paint_producer.ok_or(LayoutQueryError::PaintProducerUnavailable)?;
    let scene = producer(params.viewport.width, params.viewport.height);
    Ok(build_layout_node(&scene, "/0"))
}

/// Recursive walk: turn a `Scene` sub-tree into a [`LayoutNode`].
/// `path_prefix` is the address of `scene` within the response root
/// (`"/0"` for top-level, `"/0/1"` for the second child of root, etc.).
fn build_layout_node(scene: &Scene, path_prefix: &str) -> LayoutNode {
    let (kind, rect, tag, content, children_scenes) = describe_scene(scene);
    let children = children_scenes
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let child_path = format!("{path_prefix}/{i}");
            build_layout_node(child, &child_path)
        })
        .collect();
    LayoutNode {
        path: path_prefix.to_string(),
        kind,
        rect,
        tag,
        content,
        children,
    }
}

/// Project a [`Scene`] node into the response shape. Returns
/// `(kind, rect, tag, content, children)`. The `children` slice is
/// non-empty only for `Scene::Container`; other variants are leaves.
fn describe_scene(
    scene: &Scene,
) -> (LayoutKind, LayoutRect, Option<String>, Option<String>, &[Scene]) {
    match scene {
        Scene::Container(c) => (
            LayoutKind::Container,
            to_layout_rect(c.rect),
            c.tag.as_ref().map(ToString::to_string),
            None,
            c.children.as_slice(),
        ),
        Scene::Box(b) => (
            LayoutKind::Box,
            to_layout_rect(b.rect),
            b.tag.as_ref().map(ToString::to_string),
            None,
            &[],
        ),
        Scene::Text(t) => (
            LayoutKind::Text,
            to_layout_rect(t.rect),
            t.tag.as_ref().map(ToString::to_string),
            Some(t.content.clone()),
            &[],
        ),
        Scene::Path(p) => (
            LayoutKind::Path,
            to_layout_rect(p.rect),
            p.tag.as_ref().map(ToString::to_string),
            None,
            &[],
        ),
        Scene::Image(i) => (
            LayoutKind::Image,
            to_layout_rect(i.rect),
            i.tag.as_ref().map(ToString::to_string),
            None,
            &[],
        ),
        Scene::External(e) => (
            LayoutKind::External,
            to_layout_rect(e.rect),
            e.tag.as_ref().map(ToString::to_string),
            None,
            &[],
        ),
        Scene::Effect(_) => (
            LayoutKind::Effect,
            LayoutRect { x: 0, y: 0, w: 0, h: 0 },
            None,
            None,
            &[],
        ),
        // Forward-compatibility for future `#[non_exhaustive]` Scene
        // additions — report as Unknown rather than panicking so the
        // AI client can detect newer-than-known variants explicitly.
        _ => (
            LayoutKind::Unknown,
            LayoutRect { x: 0, y: 0, w: 0, h: 0 },
            None,
            None,
            &[],
        ),
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
            viewport: ViewportSize { width: w, height: h },
            path: None,
        }
    }

    #[test]
    fn layout_query_requires_paint_producer() {
        let params = make_params(320, 200);
        let err = layout_query::<dyn FnMut(u32, u32) -> Scene>(&params, None).unwrap_err();
        assert_eq!(err, LayoutQueryError::PaintProducerUnavailable);
    }

    #[test]
    fn layout_query_rejects_zero_viewport() {
        let params = LayoutQueryParams {
            viewport: ViewportSize { width: 0, height: 200 },
            path: None,
        };
        let mut producer = |_w: u32, _h: u32| -> Scene {
            Scene::Container(ContainerNode::new(vec![]))
        };
        let err = layout_query(&params, Some(&mut producer)).unwrap_err();
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
        let node = layout_query(&params, Some(&mut producer)).unwrap();
        assert_eq!(node.path, "/0");
        assert_eq!(node.kind, LayoutKind::Container);
        assert_eq!(node.children.len(), 1);
        let child = &node.children[0];
        assert_eq!(child.path, "/0/0");
        assert_eq!(child.kind, LayoutKind::Box);
        assert_eq!(child.tag.as_deref(), Some("button"));
        assert_eq!(child.rect, LayoutRect { x: 10, y: 20, w: 30, h: 40 });
    }

    #[test]
    fn layout_query_text_node_exposes_content_and_rect() {
        // R47.7.1 — wrap diagnosis primitive: AI inspects text rect.h
        // to detect single/multi-line wrap. content surfaces so the
        // tree dump is self-describing without re-querying state.
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
        let node = layout_query(&params, Some(&mut producer)).unwrap();
        let text = &node.children[0];
        assert_eq!(text.kind, LayoutKind::Text);
        assert_eq!(text.content.as_deref(), Some("Click me!"));
        assert_eq!(text.rect, LayoutRect { x: 50, y: 30, w: 60, h: 22 });
        assert_eq!(text.tag.as_deref(), Some("label"));
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
            Scene::Container(ContainerNode::new(vec![]).with_layout(
                LayoutStyle::new().with_size(Size::px(w, h)),
            ))
        };
        let _ = layout_query(&params, Some(&mut producer)).unwrap();
        assert_eq!(captured.get(), (345, 200));
    }
}
