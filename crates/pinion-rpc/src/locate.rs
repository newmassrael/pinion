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
//! [`crate::dispatch`](fn@crate::dispatch); this module exposes the typed dispatcher only
//! so the same logic is reusable from non-JSON-RPC carriers (gRPC,
//! direct in-process bindings).

use pinion_core::Scene;
use pinion_core::app::App;
use pinion_core::scene::{HitPath, Rect};

use crate::resolve::lookup_addressed;

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

/// Successful region-select outcome (§5.32 R39.2). Aggregates all
/// primitives whose rect intersects the query rect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocateRegionOutcome {
    /// Fully-qualified paths of every intersecting primitive,
    /// declaration order (DFS pre-order through the scene tree).
    pub paths: Vec<String>,
    /// Fully-qualified path of the deepest common ancestor — the
    /// longest segment-prefix shared by every entry in `paths`. When
    /// `paths` is empty this is the root path
    /// (`"/window[<name>]/"`); when `paths` has a single entry, the
    /// ancestor is that entry itself.
    pub common_ancestor: String,
}

/// Reasons the typed [`locate`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocateError {
    /// The point `(x, y)` falls outside the scene root's outermost rect
    /// (or hits only an [`pinion_core::scene::EffectNode`] which
    /// is not hit-testable).
    OutOfBounds,
}

/// Reasons [`bbox`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BboxError {
    /// Path string did not start with `/`, did not contain a valid
    /// `/window[id]/` prefix, or had a malformed prefix.
    Path(crate::path::PathError),
    /// Path syntax was valid but no primitive matched the segments
    /// (out-of-range index, unknown tag, or descent through a non-
    /// container).
    UnknownPath,
}

impl From<crate::path::PathError> for BboxError {
    fn from(err: crate::path::PathError) -> Self {
        BboxError::Path(err)
    }
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

    Ok(LocateOutcome {
        path: leaf_path,
        bbox: hit.bbox,
        ancestor_paths,
    })
}

/// Reverse-lookup (§5.32 R39.3): a fully-qualified path string back
/// to its viewport-relative bounding rect. The complement of [`locate`]
/// — used by AI agents to compute screen coordinates for a `highlight`
/// overlay response after they've decided which element matters.
///
/// Accepts paths in either form:
///   * `"/window[<name>]/seg1/seg2"` — explicit window prefix
///   * `"/seg1/seg2"` — implicit prefix, resolved to the initial window
///
/// An empty path or a trailing slash after the prefix returns the
/// scene root's outermost rect.
///
/// # Errors
///
/// * [`BboxError::Path`] when the window prefix is malformed.
/// * [`BboxError::UnknownPath`] when the segments do not resolve.
pub fn bbox(scene: &Scene, raw_path: &str) -> Result<Rect, BboxError> {
    let resolved = crate::path::resolve(raw_path)?;
    let _ = resolved.window; // multi-window dispatch lands later
    let segments = parse_segments(resolved.scene_path);
    // R1484 §5.32 §2 #2 — the same addressing rule `scene/query` uses. A
    // client that reached a node by name must be able to ask this method
    // about that name; `Scene::lookup_path` returns a rect rather than a
    // node, so the alias is applied by resolving the node first.
    lookup_addressed(scene, &segments)
        .map(Scene::rect)
        .ok_or(BboxError::UnknownPath)
}

/// Split a scene-path string into segments. `""`, `"/"`, and `"//"`
/// all yield an empty segment list (root). Otherwise, splits on `/`
/// and drops empty fragments.
fn parse_segments(scene_path: &str) -> Vec<String> {
    scene_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Resolve a region select against `scene`. Unlike [`locate`], a region
/// query never errors — an empty intersection returns
/// [`LocateRegionOutcome`] with empty `paths` and the root as
/// `common_ancestor`.
///
/// Coordinates are in the same viewport-relative logical pixel space
/// as [`locate`].
#[must_use]
pub fn locate_region(scene: &Scene, x: u32, y: u32, w: u32, h: u32) -> LocateRegionOutcome {
    let hits = scene.hit_test_region(x, y, w, h);
    let window = App::initial_window();
    let window_name = App::window_name(window);

    let paths: Vec<String> = hits
        .iter()
        .map(|hit| format_path(window_name, &hit.segments))
        .collect();

    // Longest common prefix across the segment vectors.
    let common_segments = longest_common_prefix(hits.iter().map(|h| h.segments.as_slice()));
    let common_ancestor = format_path(window_name, &common_segments);

    LocateRegionOutcome {
        paths,
        common_ancestor,
    }
}

/// Compute the longest segment-wise common prefix across an iterator
/// of segment slices. Returns an empty `Vec` when the iterator is
/// empty *or* when no prefix is shared.
fn longest_common_prefix<'a, I>(slices: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a [String]>,
{
    let mut iter = slices.into_iter();
    let Some(first) = iter.next() else {
        return Vec::new();
    };
    let mut prefix: Vec<String> = first.to_vec();
    for s in iter {
        let common_len = prefix
            .iter()
            .zip(s.iter())
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(common_len);
        if prefix.is_empty() {
            break;
        }
    }
    prefix
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
        assert!(
            out.path.starts_with("/window["),
            "carries explicit window prefix"
        );
        assert!(out.path.ends_with('/'), "root hit has no trailing segment");
        assert!(out.ancestor_paths.is_empty(), "root has no ancestors");
        assert_eq!(out.bbox, Rect::new(10, 10, 50, 30));
    }

    #[test]
    fn locate_returns_indexed_path_for_untagged_child() {
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![box_at(0, 0, 50, 50), box_at(100, 100, 50, 50)],
        );
        let out = locate(&s, 120, 120).expect("on child 1");
        assert!(out.path.ends_with("/1"), "got: {}", out.path);
        assert_eq!(out.ancestor_paths.len(), 1, "1 ancestor (the container)");
        assert!(out.ancestor_paths[0].ends_with('/'));
    }

    #[test]
    fn locate_uses_tag_in_path_segment() {
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![tagged_box_at(10, 10, 50, 50, "save_btn")],
        );
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

    // ---- R39.2: locate_region ----

    #[test]
    fn locate_region_disjoint_query_returns_empty_paths_and_root_ancestor() {
        let s = box_at(10, 10, 20, 20);
        let out = locate_region(&s, 500, 500, 50, 50);
        assert!(out.paths.is_empty());
        assert!(
            out.common_ancestor.ends_with('/'),
            "ancestor falls back to root"
        );
    }

    #[test]
    fn locate_region_full_overlap_collects_all_paths() {
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![box_at(10, 10, 50, 50), box_at(100, 100, 30, 30)],
        );
        let out = locate_region(&s, 0, 0, 200, 200);
        assert_eq!(out.paths.len(), 3, "container + 2 children");
        assert!(out.paths[0].ends_with('/'));
        assert!(out.paths[1].ends_with("/0"));
        assert!(out.paths[2].ends_with("/1"));
    }

    #[test]
    fn locate_region_common_ancestor_single_branch() {
        // Two children of the same container both intersect.
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![box_at(0, 0, 50, 50), box_at(100, 100, 50, 50)],
        );
        let out = locate_region(&s, 0, 0, 200, 200);
        // All three entries share the empty prefix (root container),
        // so the common ancestor is the root.
        assert!(out.common_ancestor.ends_with('/'));
    }

    #[test]
    fn locate_region_common_ancestor_nested_chain() {
        // Outer → inner → 2 boxes; query covers both boxes only.
        let inner = container_at(
            0,
            0,
            100,
            100,
            vec![box_at(10, 10, 20, 20), box_at(50, 50, 20, 20)],
        );
        let outer = container_at(0, 0, 200, 200, vec![inner]);
        let out = locate_region(&outer, 10, 10, 80, 80);
        // 4 entries: outer (empty), inner (0), box0 (0/0), box1 (0/1)
        assert_eq!(out.paths.len(), 4);
        // Common prefix across [], [0], [0,0], [0,1] is [] → root.
        assert!(out.common_ancestor.ends_with('/'));
    }

    #[test]
    fn locate_region_uses_tag_in_paths() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(10, 10, 50, 50, "btn")]);
        let out = locate_region(&s, 0, 0, 200, 200);
        assert!(out.paths.iter().any(|p| p.ends_with("/btn")));
    }

    // ---- R39.3: bbox reverse lookup ----

    #[test]
    fn bbox_empty_path_returns_root_rect() {
        let s = box_at(5, 7, 11, 13);
        assert_eq!(bbox(&s, "/window[main]").unwrap(), Rect::new(5, 7, 11, 13));
    }

    #[test]
    fn bbox_resolves_indexed_child() {
        let s = container_at(0, 0, 200, 200, vec![box_at(10, 10, 30, 30)]);
        assert_eq!(
            bbox(&s, "/window[main]/0").unwrap(),
            Rect::new(10, 10, 30, 30)
        );
    }

    #[test]
    fn bbox_resolves_tagged_child() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(20, 20, 40, 40, "btn")]);
        assert_eq!(
            bbox(&s, "/window[main]/btn").unwrap(),
            Rect::new(20, 20, 40, 40)
        );
    }

    #[test]
    fn bbox_unknown_path_returns_error() {
        let s = container_at(0, 0, 100, 100, vec![box_at(0, 0, 10, 10)]);
        assert_eq!(
            bbox(&s, "/window[main]/ghost").unwrap_err(),
            BboxError::UnknownPath
        );
    }

    #[test]
    fn bbox_implicit_window_prefix_short_circuits() {
        let s = box_at(1, 2, 3, 4);
        assert_eq!(bbox(&s, "").unwrap(), Rect::new(1, 2, 3, 4));
    }

    #[test]
    fn bbox_roundtrip_with_locate_outcome_path() {
        // locate → path; bbox(path) recovers the same rect.
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(30, 40, 50, 60, "save")]);
        let loc = locate(&s, 35, 45).expect("inside save");
        assert_eq!(bbox(&s, &loc.path).unwrap(), loc.bbox);
    }

    #[test]
    fn locate_region_zero_area_query_returns_empty() {
        let s = box_at(0, 0, 100, 100);
        let out = locate_region(&s, 50, 50, 0, 0);
        assert!(out.paths.is_empty(), "zero-area query never intersects");
    }

    #[test]
    fn locate_overlapping_siblings_picks_topmost() {
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![box_at(50, 50, 100, 100), box_at(50, 50, 100, 100)],
        );
        let out = locate(&s, 75, 75).expect("overlap");
        assert!(
            out.path.ends_with("/1"),
            "last child wins, got: {}",
            out.path
        );
    }
}
