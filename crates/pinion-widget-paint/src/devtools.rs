//! R678 §5.16 §5.49 — backend-agnostic `DevTools` substrate (path
//! addressing + selection / hover highlight overlay composition).
//!
//! Lifted at R678 from `examples/hello-multi-window` after the
//! highlight-overlay surface reached **2 consumers** in the same
//! binding — R676 selection wrap (Error red) + R678 atomic-1 hover
//! wrap (M3 `SurfaceContainerHighest`). Both consumers walked the
//! same path-stable indexing scheme (R676) and called the same wrap
//! / rebuild helpers; the second consumer fired the
//! [[abstraction-needs-second-consumer]] Rule-of-Three gate so the
//! substrate moves here next to [`crate::tree_view`].
//!
//! Future consumers of this substrate:
//!
//! * R679+ — bidirectional select (main click sets `selected_path`,
//!   inspector tree row paints focus ring on the same composite tag).
//! * R680+ — 2nd `DevTools` binding (a dedicated `pinion-devtools`
//!   example or the eventual `pinion-devtools` crate skeleton) which
//!   re-uses these helpers verbatim instead of copy-pasting.
//! * Phase D editor (R2500+) — DOM-like selection model with
//!   multiselect and selection-set diff overlays will land further
//!   substrate on top of `wrap_with_highlight` /
//!   `rebuild_with_highlight_at_path`.
//!
//! ## Path addressing scheme (CSS-selector mirror)
//!
//! Every scene node is addressable by a `/`-separated path of
//! segments. Each segment has the form `Type[disambiguator]` where
//! the disambiguator is either a tag string (`Container[main_btn]`)
//! or an nth-of-type ordinal (`Text[0]`). The root segment may
//! collapse to the bare type name (`Container`) when the root is
//! untagged — root has no siblings, so no disambiguator is needed.
//!
//! Tagged paths are **invariant to untagged-sibling churn**: if a
//! tagged node is the only one of its kind in its parent, its path
//! stays the same when untagged siblings appear or disappear around
//! it. This is the Browser-DevTools canonical Elements-panel
//! convention — Chrome / Firefox / Safari all use stable id-or-
//! ordinal selectors for the same reason.
//!
//! ## Soft-signal contract
//!
//! [`find_node_at_path`] returns `Option<&Scene>` — `None` means the
//! path does not resolve against the current scene shape. Callers
//! (highlight overlay walkers, property panes, demo verifiers) treat
//! `None` as "no match, skip the overlay / show a placeholder" and
//! continue the paint normally. This is the textbook canonical
//! contract for `DevTools`-style selection bridges where selection
//! state can carry stale or inspector-only ids that no longer
//! correspond to live scene nodes.

use std::collections::HashMap;

use pinion_core::scene::{ContainerNode, Scene};
use pinion_core::style::{Border, BoxStyle};
use pinion_core::Color;

use crate::tree_view::TreeItem;

/// R678 §5.16 §5.49 — Browser-DevTools selection overlay border
/// width in logical pixels. 2 px is the conventional Chrome /
/// Firefox / Safari inspector outline width — readable as
/// "selected" against arbitrary fills without crowding the
/// underlying paint. Public so consumers can compare the same
/// substrate constant against their painted scenes in unit tests
/// (and so the alternative-color hover wrap reads at the same
/// width as the selection wrap, keeping the dual-wrap visual
/// hierarchy consistent).
pub const DEFAULT_HIGHLIGHT_BORDER_WIDTH: u32 = 2;

/// R678 §5.16 §5.49 — disambiguator portion of a non-root path
/// segment. Tag form (`Container[main_btn]`) carries the borrowed
/// tag string; nth-of-type form (`Text[0]`) carries the parsed
/// index.
///
/// Numeric strings inside the bracket are always interpreted as
/// nth-of-type indices — tags in pinion are identifier-shaped
/// (never pure numerics in the canonical paint code), so the parser
/// disambiguates by `usize::from_str` success. This matches the
/// CSS-selector mirror convention: `:nth-of-type(N)` for ordinal
/// access, `#tag` for id access.
#[derive(Debug, PartialEq, Eq)]
pub enum PathDisambiguator<'p> {
    /// Tag form — addresses a node by its `tag` field.
    Tag(&'p str),
    /// Nth-of-type form — addresses an anonymous node by its
    /// ordinal position among siblings of the same Scene variant.
    NthOfType(usize),
}

/// R678 §5.16 §5.49 — name the [`Scene`] variant for the
/// `Type[disambiguator]` segment shape.
///
/// `#[non_exhaustive]` future variants collapse to `"Unknown"` so
/// the path scheme stays total.
#[must_use]
pub fn scene_type_name(scene: &Scene) -> &'static str {
    match scene {
        Scene::Container(_) => "Container",
        Scene::Text(_) => "Text",
        Scene::External(_) => "External",
        Scene::Box(_) => "Box",
        Scene::Path(_) => "Path",
        Scene::Image(_) => "Image",
        Scene::Effect(_) => "Effect",
        Scene::Scroll(_) => "Scroll",
        _ => "Unknown",
    }
}

/// R678 §5.16 §5.49 — tag of a [`Scene`] node when the variant
/// carries one. Only `Container` and `External` are tag-bearing;
/// every other variant returns `None` and uses the nth-of-type
/// index form in path segments.
#[must_use]
pub fn scene_tag(scene: &Scene) -> Option<&str> {
    match scene {
        Scene::Container(c) => c.tag.as_deref(),
        Scene::External(e) => e.tag.as_deref(),
        _ => None,
    }
}

/// R678 §5.16 §5.49 — root-level path segment for a [`Scene`] node.
///
/// The root has no siblings to disambiguate against, so the
/// nth-of-type index is omitted. Tagged roots keep their tag bracket
/// (`Container[main_btn]`); untagged roots collapse to the bare
/// type name (`Container`). Mirrors the leading segment of the
/// canonical `"Container/Container[main_btn]/Text[0]"` shape.
#[must_use]
pub fn scene_root_path_segment(scene: &Scene) -> String {
    let ty = scene_type_name(scene);
    match scene_tag(scene) {
        Some(tag) => format!("{ty}[{tag}]"),
        None => ty.to_owned(),
    }
}

/// R678 §5.16 §5.49 — non-root path segment for a [`Scene`] node
/// given its **nth-of-type** sibling index (0-based, counted only
/// against siblings of the same `Scene` variant — `Text[0]` and
/// `Container[0]` coexist at the same parent level).
///
/// Tagged Container/External use the tag form
/// (`Container[main_btn]`, `External[grid]`); untagged or non-
/// tag-bearing variants use the index form (`Text[0]`, `Box[2]`).
/// CSS-selector mirror: stable element ids (tags) take priority,
/// nth-of-type is the ordinal fallback for anonymous siblings.
#[must_use]
pub fn scene_child_path_segment(scene: &Scene, nth_of_type: usize) -> String {
    let ty = scene_type_name(scene);
    match scene_tag(scene) {
        Some(tag) => format!("{ty}[{tag}]"),
        None => format!("{ty}[{nth_of_type}]"),
    }
}

/// R678 §5.16 §5.49 — parse one `/`-separated path segment into a
/// `(type_name, disambiguator)` pair. Returns `None` only when the
/// bracket form is malformed (`Container[` without closing `]`).
///
/// Three shapes:
///
/// * `"Container"` — bare type, root-only. `disambiguator = None`.
/// * `"Container[main_btn]"` — tag form. `disambiguator =
///   Some(Tag("main_btn"))`.
/// * `"Text[0]"` — nth-of-type form. `disambiguator =
///   Some(NthOfType(0))`.
#[must_use]
pub fn parse_path_segment(segment: &str) -> Option<(&str, Option<PathDisambiguator<'_>>)> {
    if let Some((ty, rest)) = segment.split_once('[') {
        let disambiguator_str = rest.strip_suffix(']')?;
        let disambiguator = match disambiguator_str.parse::<usize>() {
            Ok(idx) => PathDisambiguator::NthOfType(idx),
            Err(_) => PathDisambiguator::Tag(disambiguator_str),
        };
        Some((ty, Some(disambiguator)))
    } else {
        // Bare type — valid only at the root segment.
        Some((segment, None))
    }
}

/// R678 §5.16 §5.49 — find the child of `container` matching the
/// `(type, disambiguator)` shape carried by a single non-root path
/// segment.
///
/// For tag-form disambiguators, returns the first child of the
/// requested Scene variant whose tag equals the requested tag. For
/// nth-of-type-form, returns the child at the requested index among
/// all siblings of the requested variant (tagged or untagged — the
/// counting exactly mirrors the forward walker's
/// [`scene_to_tree_item`]).
#[must_use]
pub fn find_child_in_container<'s>(
    container: &'s ContainerNode,
    ty: &str,
    disambiguator: &PathDisambiguator<'_>,
) -> Option<&'s Scene> {
    match disambiguator {
        PathDisambiguator::Tag(tag) => container.children.iter().find(|child| {
            scene_type_name(child) == ty && scene_tag(child) == Some(*tag)
        }),
        PathDisambiguator::NthOfType(idx) => container
            .children
            .iter()
            .filter(|child| scene_type_name(child) == ty)
            .nth(*idx),
    }
}

/// R678 §5.16 §5.49 — **the inverse walker.** Given a path string
/// produced by [`scene_to_tree_item`] (or [`scene_root_path_segment`]
/// for the root), descend `root` to return the matching `&Scene`
/// borrow.
///
/// Returns `None` when the path cannot be resolved against the
/// current scene shape — for example:
///
/// * inspector-only `TreeItem` ids that were never produced by
///   [`scene_to_tree_item`] (e.g. `"state"`, `"main"` placeholder
///   ids the consuming binding adds as virtual root leaves);
/// * paths into untagged Containers whose sibling shape has churned
///   (a tagged element's path stays stable, but a `Text[2]`-form
///   path can shift if untagged sibling counts change between paint
///   cycles);
/// * paths that bottom out at a non-Container variant before all
///   segments are consumed (e.g. trying to descend
///   `Container/Text[0]/Foo` when `Text[0]` is a leaf);
/// * malformed segments (`Container[` with no closing `]`).
///
/// `None` is a soft signal — callers treat it as "no match, do not
/// wrap" and continue the paint normally. This is the textbook
/// canonical contract for `DevTools`-style selection bridges:
/// selection state may carry stale or inspector-only ids; the
/// renderer's job is to gracefully skip highlight when the id no
/// longer resolves.
#[must_use]
pub fn find_node_at_path<'s>(root: &'s Scene, path: &str) -> Option<&'s Scene> {
    let mut segments = path.split('/');
    let root_seg = segments.next()?;
    let (root_ty, root_disambiguator) = parse_path_segment(root_seg)?;

    if scene_type_name(root) != root_ty {
        return None;
    }
    // The root may carry a tag disambiguator (rare but supported);
    // an nth-of-type disambiguator at the root is malformed because
    // the root has no siblings. Reject both mismatches as None per
    // the soft-signal contract documented above.
    if let Some(disambiguator) = &root_disambiguator {
        match disambiguator {
            PathDisambiguator::Tag(tag) => {
                if scene_tag(root) != Some(*tag) {
                    return None;
                }
            }
            PathDisambiguator::NthOfType(_) => return None,
        }
    }

    let mut current = root;
    for segment in segments {
        let (ty, disambiguator) = parse_path_segment(segment)?;
        // Non-root segments must carry a disambiguator (the forward
        // walker always emits the bracket form for non-root nodes).
        // A bare-type non-root segment is malformed; treat as None.
        let disambiguator = disambiguator?;
        let Scene::Container(c) = current else {
            return None;
        };
        current = find_child_in_container(c, ty, &disambiguator)?;
    }
    Some(current)
}

/// R678 §5.16 §5.49 — wrap `scene` in a transparent
/// [`Scene::Container`] carrying a stroked border in `color`. The
/// inner scene paints normally; the outer wrapper contributes only
/// the [`DEFAULT_HIGHLIGHT_BORDER_WIDTH`] border outline that flags
/// the node as "highlighted" in the `DevTools` selection or hover
/// bridge.
///
/// **Why a wrapper container and not a [`BoxStyle`] mutation on the
/// node itself?** The selection overlay must be non-destructive: a
/// future round may want to highlight a `Scene::Text` leaf, or a
/// `Scene::External` whose `BoxStyle` is owned by the External
/// (immutable from the binding). Wrapping is the canonical
/// non-destructive composition path — the underlying node keeps its
/// paint exactly, the wrapper only adds the border outline + an
/// auto-sized layout sidecar inheriting from the child.
///
/// The wrapper uses `Default` `LayoutStyle` so it does not force any
/// flex / size constraints onto the child; the child's own layout
/// drives the final size, and the wrapper's `Rect` is computed by
/// `pinion-runtime`'s layout pass to fit the child's bounding box.
#[must_use]
pub fn wrap_with_highlight(scene: Scene, color: Color) -> Scene {
    Scene::Container(ContainerNode::new(vec![scene]).with_style(
        BoxStyle::default().with_border(Border::new(color, DEFAULT_HIGHLIGHT_BORDER_WIDTH)),
    ))
}

/// R678 §5.16 §5.49 — **the path-stable rebuild walker.** Given a
/// scene root and a path string produced by [`scene_to_tree_item`] /
/// [`scene_root_path_segment`], descend the scene by-value and wrap
/// the matching node with [`wrap_with_highlight`].
///
/// Returns the scene unchanged when the path does not resolve (see
/// [`find_node_at_path`] for the matching `&Scene`-borrow inverse
/// walker — both helpers share the soft-signal contract). Mirrors
/// the CSS-selector descent semantics of [`find_node_at_path`]: root
/// segment may be bare type or tag form; non-root segments must
/// carry a disambiguator.
///
/// The walker takes `scene` by value rather than by `&mut Scene`
/// because the matching subtree is **moved** into the wrapper and
/// replaced with the wrapper at the original child slot; mutating
/// `&mut Scene` in place would require `mem::replace` ceremony for
/// a transient placeholder. By-value descent + per-step ownership
/// transfer is the cleaner Rust idiom for tree rewrites of this
/// shape.
#[must_use]
pub fn rebuild_with_highlight_at_path(scene: Scene, path: &str, color: Color) -> Scene {
    let mut segments = path.split('/');
    let Some(root_seg) = segments.next() else {
        return scene;
    };
    let Some((root_ty, root_disambiguator)) = parse_path_segment(root_seg) else {
        return scene;
    };
    if scene_type_name(&scene) != root_ty {
        return scene;
    }
    if let Some(disambiguator) = &root_disambiguator {
        match disambiguator {
            PathDisambiguator::Tag(tag) => {
                if scene_tag(&scene) != Some(*tag) {
                    return scene;
                }
            }
            // Root nth-of-type is malformed (root has no siblings);
            // match `find_node_at_path`'s rejection so the two
            // helpers carry the same soft-signal semantics.
            PathDisambiguator::NthOfType(_) => return scene,
        }
    }
    let remaining: Vec<&str> = segments.collect();
    if remaining.is_empty() {
        return wrap_with_highlight(scene, color);
    }
    descend_and_wrap(scene, &remaining, color)
}

/// R678 §5.16 §5.49 — recursive descent helper for
/// [`rebuild_with_highlight_at_path`]. Walks `segments` one step at
/// a time, descending into the matching child by-value and
/// rebuilding the container with the rewritten child slot. Returns
/// the scene unchanged on any soft-fail (non-Container current
/// node, malformed segment, child not found).
///
/// The nth-of-type counting **exactly mirrors** the forward
/// walker's per-type increment so a path produced by
/// [`scene_to_tree_item`] resolves to the same node on rebuild as
/// it does on [`find_node_at_path`] lookup.
#[must_use]
pub fn descend_and_wrap(scene: Scene, segments: &[&str], color: Color) -> Scene {
    let Scene::Container(mut c) = scene else {
        // Non-Container variants are leaves — can't descend into
        // them. Return the original variant unchanged so the rest
        // of the scene paints normally (the selection path is
        // stale).
        return scene;
    };
    let Some(first_segment) = segments.first() else {
        // Defensive — the caller guarantees segments.len() >= 1
        // before invoking this helper. Empty slice = no-op.
        return Scene::Container(c);
    };
    let Some((ty, Some(disambiguator))) = parse_path_segment(first_segment) else {
        // Malformed non-root segment (bare type or unparseable
        // bracket) → no match → return container unchanged.
        return Scene::Container(c);
    };
    let matching_idx: Option<usize> = match &disambiguator {
        PathDisambiguator::Tag(tag) => c.children.iter().position(|child| {
            scene_type_name(child) == ty && scene_tag(child) == Some(*tag)
        }),
        PathDisambiguator::NthOfType(idx) => {
            let mut count = 0_usize;
            c.children.iter().position(|child| {
                if scene_type_name(child) == ty {
                    if count == *idx {
                        return true;
                    }
                    count += 1;
                }
                false
            })
        }
    };
    if let Some(child_idx) = matching_idx {
        // `mem::replace` swaps the matching child out for a
        // transient empty Container, transforms the extracted child,
        // then puts the (now-wrapped) result back at the same slot.
        // Canonical Rust idiom for owning-mutating a `Vec` entry
        // through a `&mut Container` reference.
        let extracted = std::mem::replace(
            &mut c.children[child_idx],
            Scene::Container(ContainerNode::new(vec![])),
        );
        c.children[child_idx] = if segments.len() == 1 {
            wrap_with_highlight(extracted, color)
        } else {
            descend_and_wrap(extracted, &segments[1..], color)
        };
    }
    Scene::Container(c)
}

/// R678 §5.16 §5.49 — walk one `Scene` node into a [`TreeItem`]
/// carrying a **path-stable** id. Containers become branches whose
/// `label` shows the variant + tag (when tagged); text nodes become
/// leaves carrying the rendered content; every other variant
/// carries a minimal "variant name" leaf.
///
/// `path` is the accumulated path string identifying this node
/// within the parent tree. The root caller passes
/// [`scene_root_path_segment`] of the scene (`"Container"` for an
/// untagged root); recursive calls append `/{Type[disambiguator]}`
/// per child, where the disambiguator is the child's tag when
/// tag-bearing, or its nth-of-type sibling index otherwise. The
/// path scheme mirrors Browser `DevTools`' Elements panel canonical
/// (CSS-selector tag-or-nth-of-type) so a single tagged element
/// (like `main_btn`) keeps the same path regardless of untagged
/// sibling shape changes around it.
///
/// All branches default to `expanded = true` so the inspector
/// shows the full main scene tree the moment it boots; collapse +
/// multi-select are future-axis carries per
/// [[abstraction-needs-second-consumer]].
#[must_use]
pub fn scene_to_tree_item(scene: &Scene, path: &str) -> TreeItem {
    match scene {
        Scene::Container(c) => {
            let tag_segment = c
                .tag
                .as_deref()
                .map_or(String::new(), |t| format!(" [{t}]"));
            let label = format!("Container{tag_segment}");
            let mut type_counts: HashMap<&'static str, usize> = HashMap::new();
            let children: Vec<TreeItem> = c
                .children
                .iter()
                .map(|child| {
                    let ty = scene_type_name(child);
                    let nth_ref = type_counts.entry(ty).or_insert(0);
                    let nth = *nth_ref;
                    *nth_ref += 1;
                    let segment = scene_child_path_segment(child, nth);
                    let child_path = format!("{path}/{segment}");
                    scene_to_tree_item(child, &child_path)
                })
                .collect();
            TreeItem::branch(path.to_owned(), label, true, children)
        }
        Scene::Text(t) => {
            let label = format!("Text: {:?}", t.content);
            TreeItem::leaf(path.to_owned(), label)
        }
        Scene::External(e) => {
            let tag_segment = e
                .tag
                .as_deref()
                .map_or(String::new(), |t| format!(" [{t}]"));
            TreeItem::leaf(path.to_owned(), format!("External{tag_segment}"))
        }
        Scene::Box(_) => TreeItem::leaf(path.to_owned(), "Box"),
        Scene::Path(_) => TreeItem::leaf(path.to_owned(), "Path"),
        Scene::Image(_) => TreeItem::leaf(path.to_owned(), "Image"),
        Scene::Effect(_) => TreeItem::leaf(path.to_owned(), "Effect"),
        Scene::Scroll(_) => TreeItem::leaf(path.to_owned(), "Scroll"),
        // `pinion_core::Scene` is `#[non_exhaustive]`; any future
        // variant lands here as an opaque leaf so the inspector
        // never panics on an unknown node.
        _ => TreeItem::leaf(path.to_owned(), "(unknown variant)"),
    }
}

#[cfg(test)]
mod tests {
    //! R678 §5.16 §5.49 — `devtools` substrate regression suite.
    //! Lifted from `examples/hello-multi-window`'s R676
    //! `r676_path_stable_indexing_tests` and `r676_find_node_at_path_tests`
    //! modules — same behaviours, now pinned at the substrate layer
    //! so every future `DevTools` consumer inherits coverage.

    use super::*;
    use pinion_core::scene::{ContainerNode, ExternalNode, Rect, TextNode};
    use pinion_core::style::TextStyle;
    use pinion_core::Color;

    fn tagged_container(tag: &'static str, children: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(children).with_tag(tag))
    }

    fn untagged_container(children: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(children))
    }

    fn text_node(content: &str) -> Scene {
        Scene::Text(TextNode::styled(
            content,
            Rect::default(),
            TextStyle::new(),
        ))
    }

    // ---------- scene_type_name / scene_tag ----------

    #[test]
    fn r678_scene_type_name_for_container_is_container() {
        let scene = untagged_container(vec![]);
        assert_eq!(scene_type_name(&scene), "Container");
    }

    #[test]
    fn r678_scene_type_name_for_text_is_text() {
        let scene = text_node("hi");
        assert_eq!(scene_type_name(&scene), "Text");
    }

    #[test]
    fn r678_scene_tag_for_tagged_container_returns_tag() {
        let scene = tagged_container("main_btn", vec![]);
        assert_eq!(scene_tag(&scene), Some("main_btn"));
    }

    #[test]
    fn r678_scene_tag_for_untagged_container_returns_none() {
        let scene = untagged_container(vec![]);
        assert_eq!(scene_tag(&scene), None);
    }

    #[test]
    fn r678_scene_tag_for_text_node_returns_none() {
        let scene = text_node("body");
        assert_eq!(scene_tag(&scene), None);
    }

    // ---------- root segment / child segment ----------

    #[test]
    fn r678_root_segment_for_untagged_container_is_bare_type_name() {
        let scene = untagged_container(vec![]);
        assert_eq!(scene_root_path_segment(&scene), "Container");
    }

    #[test]
    fn r678_root_segment_for_tagged_container_uses_tag_form() {
        let scene = tagged_container("main_btn", vec![]);
        assert_eq!(scene_root_path_segment(&scene), "Container[main_btn]");
    }

    #[test]
    fn r678_child_segment_for_tagged_container_uses_tag_form() {
        let scene = tagged_container("main_btn", vec![]);
        assert_eq!(
            scene_child_path_segment(&scene, 0),
            "Container[main_btn]",
            "tag form takes priority over nth-of-type idx",
        );
    }

    #[test]
    fn r678_child_segment_for_untagged_text_uses_type_idx_form() {
        let scene = text_node("body");
        assert_eq!(scene_child_path_segment(&scene, 2), "Text[2]");
    }

    // ---------- parse_path_segment ----------

    #[test]
    fn r678_parse_bare_type_returns_no_disambiguator() {
        let parsed = parse_path_segment("Container").unwrap();
        assert_eq!(parsed, ("Container", None));
    }

    #[test]
    fn r678_parse_tag_form_returns_tag_disambiguator() {
        let parsed = parse_path_segment("Container[main_btn]").unwrap();
        assert_eq!(
            parsed,
            ("Container", Some(PathDisambiguator::Tag("main_btn"))),
        );
    }

    #[test]
    fn r678_parse_nth_of_type_form_returns_idx_disambiguator() {
        let parsed = parse_path_segment("Text[3]").unwrap();
        assert_eq!(parsed, ("Text", Some(PathDisambiguator::NthOfType(3))));
    }

    #[test]
    fn r678_parse_malformed_unclosed_bracket_returns_none() {
        assert!(parse_path_segment("Container[main_btn").is_none());
    }

    // ---------- find_node_at_path ----------

    #[test]
    fn r678_find_node_at_root_segment_returns_root() {
        let scene = untagged_container(vec![]);
        let found = find_node_at_path(&scene, "Container").unwrap();
        assert!(matches!(found, Scene::Container(_)));
    }

    #[test]
    fn r678_find_node_at_tagged_root_returns_root() {
        let scene = tagged_container("main_btn", vec![]);
        let found = find_node_at_path(&scene, "Container[main_btn]").unwrap();
        assert_eq!(scene_tag(found), Some("main_btn"));
    }

    #[test]
    fn r678_find_node_tag_form_descent_returns_tagged_child() {
        let scene = untagged_container(vec![tagged_container("btn", vec![])]);
        let found = find_node_at_path(&scene, "Container/Container[btn]").unwrap();
        assert_eq!(scene_tag(found), Some("btn"));
    }

    #[test]
    fn r678_find_node_nth_of_type_descent_returns_matching_child() {
        let scene = untagged_container(vec![text_node("a"), text_node("b"), text_node("c")]);
        let found = find_node_at_path(&scene, "Container/Text[1]").unwrap();
        if let Scene::Text(t) = found {
            assert_eq!(t.content, "b");
        } else {
            panic!("expected Text node");
        }
    }

    #[test]
    fn r678_find_node_stale_path_returns_none() {
        let scene = untagged_container(vec![text_node("a")]);
        assert!(find_node_at_path(&scene, "Container/Container[ghost]").is_none());
    }

    #[test]
    fn r678_find_node_inspector_only_id_returns_none() {
        let scene = untagged_container(vec![]);
        assert!(find_node_at_path(&scene, "state").is_none());
        assert!(find_node_at_path(&scene, "main").is_none());
    }

    #[test]
    fn r678_find_node_root_with_nth_of_type_disambiguator_returns_none() {
        // Root has no siblings — nth-of-type at the root is
        // malformed per the soft-signal contract.
        let scene = untagged_container(vec![]);
        assert!(find_node_at_path(&scene, "Container[0]").is_none());
    }

    #[test]
    fn r678_find_node_path_through_leaf_returns_none() {
        let scene = untagged_container(vec![text_node("leaf")]);
        // Can't descend through a Text leaf.
        assert!(find_node_at_path(&scene, "Container/Text[0]/Foo[0]").is_none());
    }

    // ---------- wrap_with_highlight ----------

    #[test]
    fn r678_wrap_with_highlight_adds_outer_container_with_stroked_border() {
        let inner = tagged_container("btn", vec![]);
        let wrapped = wrap_with_highlight(inner, Color::rgb(255, 0, 0));
        let Scene::Container(c) = wrapped else {
            panic!("wrap_with_highlight must produce a Container");
        };
        assert_eq!(c.children.len(), 1, "wrapper carries exactly one child");
        let border = c.style.border.expect("wrapper must carry a Border");
        assert_eq!(border.width, DEFAULT_HIGHLIGHT_BORDER_WIDTH);
        assert_eq!(border.color, Color::rgb(255, 0, 0));
    }

    // ---------- rebuild_with_highlight_at_path ----------

    #[test]
    fn r678_rebuild_with_highlight_at_root_wraps_root() {
        let scene = untagged_container(vec![text_node("body")]);
        let rebuilt = rebuild_with_highlight_at_path(scene, "Container", Color::rgb(0, 0, 255));
        // Root is now the wrapper; the inner child is the original
        // root.
        let Scene::Container(outer) = rebuilt else {
            panic!("expected wrapper Container");
        };
        assert!(outer.style.border.is_some_and(|b| b.width > 0));
        assert_eq!(outer.children.len(), 1);
    }

    #[test]
    fn r678_rebuild_with_highlight_at_tagged_descendant_wraps_in_place() {
        let scene = untagged_container(vec![tagged_container("btn", vec![text_node("Click")])]);
        let rebuilt = rebuild_with_highlight_at_path(
            scene,
            "Container/Container[btn]",
            Color::rgb(255, 0, 0),
        );
        // Outer = original root; root's child[0] is now the wrapper.
        let Scene::Container(root) = rebuilt else {
            panic!("expected Container root");
        };
        assert_eq!(root.children.len(), 1);
        let wrapper = &root.children[0];
        let Scene::Container(wrap) = wrapper else {
            panic!("child[0] must now be the wrapper Container");
        };
        assert!(wrap.style.border.is_some_and(|b| b.width > 0));
        assert_eq!(wrap.children.len(), 1);
        // Wrapper's single child is the original tagged container.
        if let Scene::Container(inner) = &wrap.children[0] {
            assert_eq!(inner.tag.as_deref(), Some("btn"));
        } else {
            panic!("wrapper's child must be the original tagged container");
        }
    }

    #[test]
    fn r678_rebuild_with_highlight_at_stale_path_returns_scene_unchanged() {
        let scene = untagged_container(vec![text_node("body")]);
        let rebuilt = rebuild_with_highlight_at_path(
            scene,
            "Container/Container[ghost]",
            Color::rgb(255, 0, 0),
        );
        // No wrap was applied — root carries no border.
        let Scene::Container(root) = rebuilt else {
            panic!("expected Container root");
        };
        assert!(
            root.style.border.is_none() || root.style.border.is_some_and(|b| b.width == 0),
            "stale path must not introduce a border",
        );
    }

    // ---------- scene_to_tree_item ----------

    #[test]
    fn r678_scene_to_tree_item_walks_container_into_branch() {
        let scene = untagged_container(vec![text_node("a"), text_node("b")]);
        let tree = scene_to_tree_item(&scene, "Container");
        assert_eq!(tree.id, "Container");
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].id, "Container/Text[0]");
        assert_eq!(tree.children[1].id, "Container/Text[1]");
    }

    #[test]
    fn r678_scene_to_tree_item_tagged_child_uses_tag_form_in_path() {
        let scene = untagged_container(vec![tagged_container("btn", vec![])]);
        let tree = scene_to_tree_item(&scene, "Container");
        assert_eq!(tree.children[0].id, "Container/Container[btn]");
    }

    #[test]
    fn r678_scene_to_tree_item_external_leaf_carries_tag() {
        let ext = Scene::External(
            ExternalNode::new(Box::new(NoopExternal)).with_tag("inspector_tree"),
        );
        let scene = untagged_container(vec![ext]);
        let tree = scene_to_tree_item(&scene, "Container");
        assert_eq!(tree.children[0].id, "Container/External[inspector_tree]");
        assert_eq!(tree.children[0].label, "External [inspector_tree]");
    }

    // ---------- mixed-type sibling per-type idx pin ----------

    #[test]
    fn r678_mixed_type_siblings_have_per_type_idx() {
        // 2 Text nodes interleaved with 1 Container — Text gets
        // indices 0/1, Container gets index 0.
        let scene = untagged_container(vec![
            text_node("a"),
            untagged_container(vec![]),
            text_node("b"),
        ]);
        let tree = scene_to_tree_item(&scene, "Container");
        assert_eq!(tree.children[0].id, "Container/Text[0]");
        assert_eq!(tree.children[1].id, "Container/Container[0]");
        assert_eq!(tree.children[2].id, "Container/Text[1]");
    }

    // ---------- tagged path stability across untagged sibling churn ----------

    #[test]
    fn r678_tagged_path_is_stable_when_untagged_sibling_appears() {
        // Before: tagged Container is the only child.
        // After: an untagged Text appears above it. Tagged path
        // stays identical.
        let before = untagged_container(vec![tagged_container("btn", vec![])]);
        let after = untagged_container(vec![
            text_node("new banner"),
            tagged_container("btn", vec![]),
        ]);

        let path = "Container/Container[btn]";
        assert!(find_node_at_path(&before, path).is_some());
        assert!(find_node_at_path(&after, path).is_some());
    }

    /// Test-only stub `External` for the path scheme tests — we
    /// need a concrete `dyn External` to construct an
    /// [`ExternalNode`], but the substrate tests don't exercise
    /// its behaviour. The substrate's main tests under
    /// `crate::tree_view::r675_tree_row_click_external_tests` /
    /// `r678_tree_row_hover_external_tests` cover the real
    /// External contract.
    #[derive(Debug, Default)]
    struct NoopExternal;

    impl pinion_core::external::External for NoopExternal {
        fn backends(&self) -> pinion_core::external::BackendSupport {
            pinion_core::external::BackendSupport::new(
                &[pinion_core::external::Backend::Gui],
                pinion_core::external::BackendFallback::Skip,
            )
        }

        fn repaint_ownership(&self) -> pinion_core::external::RepaintOwner {
            pinion_core::external::RepaintOwner::Framework
        }

        fn thread_ownership(&self) -> pinion_core::external::ThreadOwnership {
            pinion_core::external::ThreadOwnership::UiThreadSync
        }
    }
}
