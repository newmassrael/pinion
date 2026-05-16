//! `scene/locate` RPC method dispatch (§5.32 R39 v0).
//!
//! Spatial → semantic bridge: takes viewport coordinates `(x, y)` and
//! returns the deepest scene-tree primitive whose rect contains the
//! point, along with its ancestor chain. This is the first-class
//! alternative to screenshot-OCR — the AI agent receives a structured
//! `path` + `bbox` it can immediately reason over, without going
//! through pixels.
//!
//! Path syntax (v0):
//!
//! ```text
//! /[window[id]/]segment[/segment ...]
//! ```
//!
//! Each segment is either a positional index (`"3"`) or the child's
//! §5.20 tag (`"save_btn"`) per [`Scene::hit_test`] semantics. The
//! `/window[id]/` prefix is always emitted, even for the implicit
//! initial window — fully-qualified paths are easier for downstream
//! tooling to dispatch on.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) lives in
//! [`crate::dispatch`]; this module exposes the typed dispatcher only
//! so the same logic is reusable from non-JSON-RPC carriers (gRPC,
//! direct in-process bindings).

use pinion_core::Scene;
use pinion_core::app::App;
use pinion_core::scene::{HitPath, Rect};

/// Successful hit-test outcome (§5.32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocateOutcome {
    /// Fully-qualified path of the deepest matched primitive, including
    /// the `/window[id]/` prefix.
    pub path: String,
    /// Bounding rect of the deepest match — viewport-relative.
    pub bbox: Rect,
    /// Paths of every ancestor between the scene root and the match,
    /// in root-first order. Empty when the match itself is at the root.
    pub ancestor_paths: Vec<String>,
}

/// Reasons the typed [`locate`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocateError {
    /// The point `(x, y)` falls outside the scene root's outermost rect
    /// (or hits only an [`crate::pinion_core::scene::EffectNode`] which
    /// is not hit-testable).
    OutOfBounds,
}

/// Resolve `(x, y)` against `scene` to a [`LocateOutcome`].
///
/// Coordinates are in the same viewport-relative logical pixel space
/// as the scene's `Rect` values (§5.32: DPI-independent CSS px; physical
/// device px conversion is the backend's responsibility, not pinion-rpc's).
///
/// # Errors
///
/// Returns [`LocateError::OutOfBounds`] when no primitive contains the
/// point. Path resolution does not error at this entry point — there is
/// no window-prefix input to parse (the result *carries* the prefix).
pub fn locate(scene: &Scene, x: u32, y: u32) -> Result<LocateOutcome, LocateError> {
    let hit: HitPath = scene.hit_test(x, y).ok_or(LocateError::OutOfBounds)?;
    let window = App::initial_window();
    let window_name = App::window_name(window);

    let leaf_path = format_path(window_name, &hit.segments);
    let ancestor_paths = (0..hit.segments.len())
        .map(|i| format_path(window_name, &hit.segments[..i]))
        .collect();

    Ok(LocateOutcome { path: leaf_path, bbox: hit.bbox, ancestor_paths })
}

/// Build a fully-qualified path string. Empty `segments` yields
/// `"/window[<name>]/"`; `["a", "b"]` yields `"/window[<name>]/a/b"`.
fn format_path(window_name: &str, segments: &[String]) -> String {
    let mut out = String::with_capacity(window_name.len() + 16);
    out.push_str("/window[");
    out.push_str(window_name);
    out.push(']');
    out.push('/');
    if !segments.is_empty() {
        out.push_str(&segments.join("/"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, ContainerNode};
    use pinion_core::style::Color;

    fn box_at(x: u32, y: u32, w: u32, h: u32) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()))
    }

    fn tagged_box_at(x: u32, y: u32, w: u32, h: u32, tag: &'static str) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()).with_tag(tag))
    }

    fn container_at(x: u32, y: u32, w: u32, h: u32, children: Vec<Scene>) -> Scene {
        let mut node = ContainerNode::new(children);
        node.rect = Rect::new(x, y, w, h);
        Scene::Container(node)
    }

    #[test]
    fn locate_returns_window_prefixed_path_for_root_hit() {
        let s = box_at(10, 10, 50, 30);
        let out = locate(&s, 20, 15).expect("inside");
        assert!(out.path.starts_with("/window["), "carries explicit window prefix");
        assert!(out.path.ends_with('/'), "root hit has no trailing segment");
        assert!(out.ancestor_paths.is_empty(), "root has no ancestors");
        assert_eq!(out.bbox, Rect::new(10, 10, 50, 30));
    }

    #[test]
    fn locate_returns_indexed_path_for_untagged_child() {
        let s = container_at(0, 0, 200, 200, vec![box_at(0, 0, 50, 50), box_at(100, 100, 50, 50)]);
        let out = locate(&s, 120, 120).expect("on child 1");
        assert!(out.path.ends_with("/1"), "got: {}", out.path);
        assert_eq!(out.ancestor_paths.len(), 1, "1 ancestor (the container)");
        assert!(out.ancestor_paths[0].ends_with('/'));
    }

    #[test]
    fn locate_uses_tag_in_path_segment() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(10, 10, 50, 50, "save_btn")]);
        let out = locate(&s, 20, 20).expect("tagged hit");
        assert!(out.path.ends_with("/save_btn"), "got: {}", out.path);
    }

    #[test]
    fn locate_ancestor_chain_is_root_first() {
        // Outer at (0,0,200,200) → inner at (0,0,100,100) → box at (10,10,50,50)
        let inner = container_at(0, 0, 100, 100, vec![box_at(10, 10, 50, 50)]);
        let outer = container_at(0, 0, 200, 200, vec![inner]);
        let out = locate(&outer, 20, 20).expect("deep nested");
        // Path = /window[name]/0/0
        assert_eq!(out.path.matches('/').count(), 3, "got: {}", out.path);
        // ^ already uses char literal — kept as-is
        // Ancestors = [outer-container-path, inner-container-path]
        assert_eq!(out.ancestor_paths.len(), 2);
        // Root-first: outer (no segments) precedes inner (one segment).
        assert!(out.ancestor_paths[0].ends_with('/'));
        assert!(out.ancestor_paths[1].ends_with("/0"));
    }

    #[test]
    fn locate_out_of_bounds_returns_error() {
        let s = box_at(10, 10, 50, 30);
        assert_eq!(locate(&s, 5, 5).unwrap_err(), LocateError::OutOfBounds);
    }

    #[test]
    fn locate_overlapping_siblings_picks_topmost() {
        let s = container_at(
            0, 0, 200, 200,
            vec![box_at(50, 50, 100, 100), box_at(50, 50, 100, 100)],
        );
        let out = locate(&s, 75, 75).expect("overlap");
        assert!(out.path.ends_with("/1"), "last child wins, got: {}", out.path);
    }
}
