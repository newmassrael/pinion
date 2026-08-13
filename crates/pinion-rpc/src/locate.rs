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
use pinion_core::region::{Region, RegionError, RegionFit};
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

/// Successful region-select outcome (§5.32 R39.2, generalised R1591).
/// Aggregates every primitive the region covers, and repeats the question.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LocateRegionOutcome {
    /// Fully-qualified paths of every covered primitive, declaration order
    /// (DFS pre-order through the scene tree).
    pub paths: Vec<String>,
    /// Fully-qualified path of the deepest common ancestor — the
    /// longest segment-prefix shared by every entry in `paths`. When
    /// `paths` is empty this is the root path
    /// (`"/window[<name>]/"`); when `paths` has a single entry, the
    /// ancestor is that entry itself.
    pub common_ancestor: String,
    /// Which shape the question was asked with — `"rect"`, `"circle"` or
    /// `"lasso"` (R1591).
    ///
    /// Repeated back because a selection's answer is only interpretable
    /// alongside what was asked, and because the toolkit's rubber band takes
    /// its mode from a **view property** that nothing records per selection.
    pub shape: String,
    /// Which fit was applied — `"intersects"` or `"contains"` (R1591).
    pub fit: String,
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
    bbox_of(scene, raw_path).map(|outcome| outcome.bbox)
}

/// R1653 §5.32 §5.45 §2 #7 — [`bbox`], and **where that rectangle is on
/// screen**.
///
/// The two are the same number for most of a scene and not for any of it that
/// lives inside a [`Scene::Scroll`], whose content rects are scroll-local. That
/// difference is not a detail: `bbox`'s stated purpose is that an agent
/// "compute screen coordinates" from it, and inside a scrolled pane those
/// coordinates address nothing. Measured the day this was written — a node
/// graph whose canvas became a viewport reported a card at `(2207, 2081)` in a
/// 1440x900 window, and the agent's click was refused as out of bounds.
///
/// So the answer carries both, named for what each one is:
///
/// * [`bbox`](BboxOutcome::bbox) — the node's own rectangle, in the frame its
///   container states it in. Unchanged, and still what a caller reasoning about
///   a subtree's internal layout wants.
/// * [`window`](BboxOutcome::window) — where it is painted, every enclosing
///   scroll offset folded in and every enclosing clip applied, or `None` when
///   the clips leave nothing of it visible. This is the one a pointer can use,
///   and it is the same resolver [`Scene::rect_for_tag_absolute`] answers click
///   routing with, so an agent that presses here presses what it measured.
///
/// Reporting both rather than replacing one with the other is deliberate: their
/// difference is a *fact* about the scene — this node is inside something that
/// scrolls — and a surface that collapses two facts into one answer makes the
/// remaining answer ambiguous.
///
/// # Errors
///
/// Identical to [`bbox`].
pub fn bbox_of(scene: &Scene, raw_path: &str) -> Result<BboxOutcome, BboxError> {
    let resolved = crate::path::resolve(raw_path)?;
    let _ = resolved.window; // multi-window dispatch lands later
    let segments = crate::path::segments(resolved.scene_path);
    // R1484 §5.32 §2 #2 — the same addressing rule `scene/query` uses. A
    // client that reached a node by name must be able to ask this method
    // about that name; `Scene::lookup_path` returns a rect rather than a
    // node, so the alias is applied by resolving the node first.
    let node = lookup_addressed(scene, &segments).ok_or(BboxError::UnknownPath)?;
    // Identity, not address: the walk's own addressing and this method's differ
    // under the aliases above, and re-deriving the address here would be a
    // second resolver free to disagree with the one that just answered.
    let mut window = None;
    scene.for_each_node(&mut |visit| {
        if std::ptr::eq(visit.node, node) && window.is_none() {
            window = Some(visit.absolute_rect());
        }
    });
    Ok(BboxOutcome {
        bbox: node.rect(),
        window: window.flatten(),
    })
}

/// R1653 §5.32 §5.20 — [`bbox_of`], located by **tag** rather than by address.
///
/// The address form needs the whole chain from the root, which a caller reading
/// a snapshot does not have in the form this method accepts — a
/// [`Scene::Scroll`] is path-transparent here and is a segment in a snapshot
/// walk, so the two spellings differ exactly where the window rect starts to
/// matter. A tag is one word and it is the same word
/// [`Scene::rect_for_tag_absolute`] routes a click by, which makes "measure it,
/// then press it" one vocabulary instead of two.
///
/// First tag wins, pre-order, the rule the whole tag surface shares.
#[must_use]
pub fn bbox_of_tag(scene: &Scene, tag: &str) -> Option<BboxOutcome> {
    let mut found = None;
    scene.for_each_node(&mut |visit| {
        if found.is_none() && visit.node.tag() == Some(tag) {
            found = Some(BboxOutcome {
                bbox: visit.node.rect(),
                window: visit.absolute_rect(),
            });
        }
    });
    found
}

/// R1676 §5.32 §5.20 §2 #7 — [`bbox_of_tag`] for **every** tag in one pass:
/// each §5.20 intent tag the scene carries, and where a pointer reaches it.
///
/// # Why an enumeration, and not N calls to [`bbox_of_tag`]
///
/// Because a process with no pointer has to ask a *different* question than a
/// person does. A person sees the screen and then aims; an agent has to learn
/// what is aimable, and asking one tag at a time requires already knowing the
/// tag — which is the same gap `rpc/methods` was built to close one layer up.
///
/// The floor was measured on the mature retained-mode toolkit at 6.11
/// (offscreen, an item view horizontally scrolled to its maximum):
///
/// * A per-item rect is reported **unclipped** — the leading column answered
///   `x=-232 w=99` against a `0..368` viewport, an intersection of nothing.
///   Visibility is a second call the caller performs itself.
/// * That toolkit's own point→item lookup then **agrees with the unclipped
///   rect**: asked for the item at the centre of a rect lying entirely left of
///   the viewport, it names the cell rather than refusing. So the two-step is
///   not merely inconvenient, it has an affirmative wrong answer in it.
/// * Its visible-region call, which does answer honestly (empty once the
///   target scrolls out), is a *widget* member — and an item-view cell is not
///   a widget, so for the case above it does not exist at all.
/// * There is no enumeration in either direction: a caller wanting every
///   addressable element walks the widget tree itself, and for a model-backed
///   view walks the model.
///
/// Here `window` is the same fact [`NodeVisit::absolute_rect`] answers, so
/// reported and visible cannot come apart, it is answered for every mark
/// rather than only for the ones that happen to be widgets, and the whole tree
/// costs one round trip.
///
/// # The `None` window is carried, not dropped
///
/// A tag whose node is painted but wholly clipped away is **present** with
/// `window: None`. Dropping it would make "this screen has no such mark" and
/// "this mark is scrolled out of sight" the same answer, and they take
/// different repairs — the first is a defect in the view, the second is a
/// scroll away from being fixed. [`crate::scroll_reach`] is the method that
/// tells those two apart; this one must not erase the distinction before it
/// gets there.
///
/// First tag wins, pre-order, the rule the whole tag surface shares — the same
/// rule [`bbox_of_tag`] and [`Scene::absolute_rects_by_tag`] apply, and it is
/// applied here by the same walk rather than by a second copy of it.
///
/// [`NodeVisit::absolute_rect`]: pinion_core::scene::NodeVisit::absolute_rect
#[must_use]
pub fn tag_rects_of(scene: &Scene) -> Vec<(String, BboxOutcome)> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<(String, BboxOutcome)> = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Some(tag) = visit.node.tag() else { return };
        if !seen.insert(tag.to_owned()) {
            return;
        }
        out.push((
            tag.to_owned(),
            BboxOutcome {
                bbox: visit.node.rect(),
                window: visit.absolute_rect(),
            },
        ));
    });
    out
}

/// What [`bbox_of`] answers: a node's own rectangle, and where it is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BboxOutcome {
    /// The node's own `rect`, in whatever frame its container states it in.
    pub bbox: Rect,
    /// Where it is painted, scroll offsets folded and clips applied, or `None`
    /// when nothing of it is visible.
    pub window: Option<Rect>,
}

/// R1591 §5.32 §2 #7 — resolve any [`Region`] against `scene` under a
/// [`RegionFit`].
///
/// The one region question: a rectangle, a disc and a lasso are one question,
/// asked from outside the process by something that has no pointer. The DCC
/// answers the same three with three operators, and the toolkit's equivalent
/// lives behind a painter path that cannot leave the process at all.
///
/// The outcome **repeats what was asked** — see [`LocateRegionOutcome::shape`]
/// and [`LocateRegionOutcome::fit`].
///
/// # Errors
///
/// [`RegionError`] for a shape that bounds no area, so a two-point lasso is told apart
/// from an empty surface. The toolkit's `items(<polygon>, ..)` answers both with an empty list.
pub fn locate_shape(
    scene: &Scene,
    region: &Region,
    fit: RegionFit,
) -> Result<LocateRegionOutcome, RegionError> {
    let hits = scene.hit_test_shape(region, fit)?;
    let window = App::initial_window();
    let window_name = App::window_name(window);

    let paths: Vec<String> = hits
        .iter()
        .map(|hit| format_path(window_name, &hit.segments))
        .collect();

    // Longest common prefix across the segment vectors.
    let common_segments = longest_common_prefix(hits.iter().map(|h| h.segments.as_slice()));
    let common_ancestor = format_path(window_name, &common_segments);

    Ok(LocateRegionOutcome {
        paths,
        common_ancestor,
        shape: region.kind().to_owned(),
        fit: fit.to_string(),
    })
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
/// (R1557) `pub(crate)` — `scene/draw_profile` renders the same address for
/// every attributed node, and a profile row's path resolving in
/// `scene/snapshot` is the point of publishing it. Two formatters would be two
/// chances to disagree about what an address looks like.
pub(crate) fn format_path(window_name: &str, segments: &[impl AsRef<str>]) -> String {
    let mut out = String::with_capacity(window_name.len() + 16);
    out.push_str("/window[");
    out.push_str(window_name);
    out.push(']');
    out.push('/');
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(seg.as_ref());
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
        let out = locate_shape(&s, &Region::rect(500, 500, 50, 50), RegionFit::Intersects).unwrap();
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
        let out = locate_shape(&s, &Region::rect(0, 0, 200, 200), RegionFit::Intersects).unwrap();
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
        let out = locate_shape(&s, &Region::rect(0, 0, 200, 200), RegionFit::Intersects).unwrap();
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
        let out =
            locate_shape(&outer, &Region::rect(10, 10, 80, 80), RegionFit::Intersects).unwrap();
        // 4 entries: outer (empty), inner (0), box0 (0/0), box1 (0/1)
        assert_eq!(out.paths.len(), 4);
        // Common prefix across [], [0], [0,0], [0,1] is [] → root.
        assert!(out.common_ancestor.ends_with('/'));
    }

    #[test]
    fn locate_region_uses_tag_in_paths() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(10, 10, 50, 50, "btn")]);
        let out = locate_shape(&s, &Region::rect(0, 0, 200, 200), RegionFit::Intersects).unwrap();
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
    fn locate_region_zero_area_query_is_now_named_rather_than_answered_empty() {
        // R1591 changed this contract deliberately, and it is the round's own
        // argument: "your rectangle had no area" and "nothing is there" were
        // the same answer, so a caller could not tell a bad gesture from an
        // empty canvas. The toolkit cannot tell them apart either — `items()`
        // returns a list.
        let s = box_at(0, 0, 100, 100);
        assert_eq!(
            locate_shape(&s, &Region::rect(50, 50, 0, 0), RegionFit::Intersects),
            Err(RegionError::Empty)
        );
        // The empty ANSWER is still reachable, and now means only itself.
        let away = locate_shape(&s, &Region::rect(500, 500, 10, 10), RegionFit::Intersects)
            .expect("a legal region");
        assert!(away.paths.is_empty());
        assert_eq!(away.shape, "rect");
        assert_eq!(away.fit, "intersects");
    }

    #[test]
    fn r1591_the_outcome_repeats_what_was_asked() {
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![
                tagged_box_at(10, 10, 20, 20, "near"),
                tagged_box_at(150, 150, 20, 20, "far"),
            ],
        );
        let disc = Region::circle(20, 20, 30);
        let out = locate_shape(&s, &disc, RegionFit::Contains).expect("a legal region");
        assert_eq!(out.shape, "circle");
        assert_eq!(
            out.fit, "contains",
            "PAST THE FLOOR — the mode a selection used is part of its answer. The toolkit takes \
             it from rubberBandSelectionMode, a view property \
             that nothing records per selection"
        );
        assert!(out.paths.iter().any(|p| p.ends_with("/near")));
        assert!(!out.paths.iter().any(|p| p.ends_with("/far")));
    }

    /// ★ R1653 — inside a scroll, "its rectangle" and "where it is on screen"
    /// are two different numbers, and only one of them is a place a pointer can
    /// go. This is the case that broke a real agent: a card reported at
    /// `(2207, 2081)` in a 1440x900 window, clicked, refused as out of bounds.
    #[test]
    fn r1653_bbox_reports_the_rectangle_and_where_it_is_painted() {
        let content = container_at(
            0,
            0,
            400,
            400,
            vec![
                tagged_box_at(20, 30, 40, 20, "near"),
                tagged_box_at(20, 300, 40, 20, "far"),
            ],
        );
        let mut scroll = pinion_core::scene::ScrollNode::new(Rect::new(100, 50, 200, 100), content);
        scroll.offset_y = 10;
        let scene = container_at(0, 0, 500, 500, vec![Scene::Scroll(scroll)]);

        let near = bbox_of(&scene, "/0/near").expect("addressed");
        assert_eq!(
            near.bbox,
            Rect::new(20, 30, 40, 20),
            "its own rectangle is scroll-local and says so"
        );
        assert_eq!(
            near.window,
            Some(Rect::new(120, 70, 40, 20)),
            "★ and where it is painted folds the viewport origin and the offset"
        );

        let far = bbox_of(&scene, "/0/far").expect("addressed");
        assert_eq!(far.bbox, Rect::new(20, 300, 40, 20));
        assert_eq!(
            far.window, None,
            "★ scrolled past the viewport: painted nowhere, and the answer says              so rather than naming a rectangle nothing was drawn in"
        );

        // And the answer agrees with the resolver click routing uses.
        assert_eq!(near.window, scene.rect_for_tag_absolute("near"));
        assert_eq!(far.window, scene.rect_for_tag_absolute("far"));
    }

    /// Outside a scroll the two answers are the same, which is why nothing
    /// noticed for 948 rounds.
    #[test]
    fn r1653_without_a_scroll_both_answers_are_one_rectangle() {
        let scene = container_at(0, 0, 200, 200, vec![tagged_box_at(50, 60, 30, 10, "plain")]);
        let out = bbox_of(&scene, "/plain").expect("addressed");
        assert_eq!(out.bbox, Rect::new(50, 60, 30, 10));
        assert_eq!(out.window, Some(out.bbox));
    }

    /// ★★ R1676 — the enumeration answers the same thing the single lookup
    /// does, for every tag, including the ones the single lookup would have to
    /// be asked about by name to learn they exist.
    ///
    /// Four cases in one scene because they are four different ways the two
    /// could come apart: a mark fully in view, a mark the viewport CUTS, a mark
    /// scrolled wholly out, and a tag used twice. The last is the one a fixture
    /// without duplicates cannot see, and it is where the tree's own two
    /// implementations disagreed before R1653 pinned them together.
    ///
    /// The load-bearing assertion is that the scrolled-out mark is PRESENT with
    /// no window rather than absent: "there is no such mark" and "it is out of
    /// sight" take different repairs, and an enumeration that drops the second
    /// hands the caller the first.
    #[test]
    fn r1676_the_enumeration_answers_what_the_single_lookup_answers() {
        let content = container_at(
            0,
            0,
            400,
            400,
            vec![
                tagged_box_at(10, 30, 40, 20, "whole"),
                tagged_box_at(80, 30, 40, 20, "cut"),
                tagged_box_at(10, 300, 40, 20, "gone"),
                tagged_box_at(10, 340, 40, 20, "twice"),
                tagged_box_at(10, 60, 40, 20, "twice"),
            ],
        );
        let mut scroll = pinion_core::scene::ScrollNode::new(Rect::new(100, 50, 100, 100), content);
        scroll.offset_y = 10;
        let scene = container_at(0, 0, 500, 500, vec![Scene::Scroll(scroll)]);

        let rows = tag_rects_of(&scene);
        let by_tag: std::collections::HashMap<&str, &BboxOutcome> =
            rows.iter().map(|(t, o)| (t.as_str(), o)).collect();

        assert_eq!(
            by_tag["whole"].window,
            Some(Rect::new(110, 70, 40, 20)),
            "a mark inside the viewport reports where it is painted"
        );
        assert_eq!(
            by_tag["cut"].window,
            Some(Rect::new(180, 70, 20, 20)),
            "★ a mark the viewport CUTS reports the part a pointer can reach, \
             not the 40px the author asked for — the half the reference \
             toolkit leaves to the caller"
        );
        assert_eq!(
            by_tag["cut"].bbox,
            Rect::new(80, 30, 40, 20),
            "and its own rectangle is still the whole of it, so nothing is lost \
             by folding the clip into the other field"
        );
        assert!(
            by_tag.contains_key("gone"),
            "★ a mark scrolled out of sight is REPORTED — dropping it would \
             answer 'no such mark', which is a different defect with a \
             different repair",
        );
        assert_eq!(
            by_tag["gone"].window, None,
            "and it says nothing is reachable"
        );
        assert_eq!(
            by_tag["twice"].window, None,
            "★ FIRST tag wins: the earlier `twice` is the scrolled-out one, and \
             a walk that let a later duplicate fill the slot would answer a \
             different node from the single lookup",
        );

        // Neither is allowed to be a second copy of the other.
        for (tag, outcome) in &rows {
            assert_eq!(
                Some(*outcome),
                bbox_of_tag(&scene, tag),
                "the enumeration and the single lookup disagree about {tag:?}",
            );
            assert_eq!(
                outcome.window,
                scene.rect_for_tag_absolute(tag),
                "and neither agrees with the resolver a click routes by, for {tag:?}",
            );
        }

        // The counter-assertion: this is an enumeration, so it must have found
        // the tags WITHOUT being told them. A helper that answered only what it
        // was asked about would pass everything above and enumerate nothing.
        let mut names: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["cut", "gone", "twice", "whole"]);
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
