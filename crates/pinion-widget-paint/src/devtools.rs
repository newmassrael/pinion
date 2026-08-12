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

use pinion_core::Color;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Scene};
use pinion_core::style::{Border, BoxStyle};

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
        PathDisambiguator::Tag(tag) => container
            .children
            .iter()
            .find(|child| scene_type_name(child) == ty && scene_tag(child) == Some(*tag)),
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
        PathDisambiguator::Tag(tag) => c
            .children
            .iter()
            .position(|child| scene_type_name(child) == ty && scene_tag(child) == Some(*tag)),
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

/// R679 §5.16 §5.49 — **the paint-side hit-test inverse walker.**
/// Given a painted scene and a hit coordinate `(x, y)`, walk the
/// scene tree following the same topmost-last (declaration-order
/// reverse iteration) descent semantics as [`Scene::hit_test`] and
/// return the [`scene_to_tree_item`]-canonical path of the **deepest
/// tagged ancestor** at that hit.
///
/// Returns `None` when:
///
/// * `(x, y)` lies outside the scene's outermost rect (no hit at all);
/// * the descent finds a hit but no node along the chain carries a
///   tag (every ancestor is anonymous — typical for an untagged
///   wrapper Container plus untagged Text leaves; the canonical
///   `DevTools` selection semantics interpret this as "nothing
///   actionable here", i.e. background-click → deselect on the
///   binding side).
///
/// ## Why deepest-**tagged** ancestor (not deepest node)?
///
/// Mirrors the runtime [`crate::tree_view`] hit-target convention +
/// the `InputRouter`'s `resolve_hover_tag` deepest-tagged-ancestor
/// semantics so a single (x, y) → path map is consistent across
/// every paint-side input arc. DevTools-style "click anywhere →
/// highlight that widget" reads naturally as "the widget under the
/// pointer" — tagged elements are the widgets the binding cares
/// about; untagged decoration (label Text inside the button, the
/// flexbox wrapper around content) is paint-implementation detail
/// the selection bridge should walk past.
///
/// The returned path is always **resolvable** through
/// [`find_node_at_path`] — this is the substrate's load-bearing
/// invariant. Every successful hit returns a path string that
/// round-trips: `find_node_at_path(scene,
/// path_for_paint_hit(scene, x, y).unwrap()).is_some()`. Unit tests
/// pin the round-trip explicitly.
///
/// ## Scroll containers (v1 carry)
///
/// The walker does **not** descend into [`Scene::Scroll`] content —
/// the Scroll variant returns the path **up to** the Scroll node
/// itself (deepest tagged ancestor is the closest tagged container
/// above the Scroll, if any). This is honest carry for R679: the
/// first DevTools-on-Scroll consumer (R680+) will lift the Scroll
/// descent into the substrate with offset translation matching
/// [`Scene::hit_test`]'s. hello-multi-window's main paint carries no
/// Scroll today, so the v1 carry does not affect the bidirectional
/// select demo.
#[must_use]
pub fn path_for_paint_hit(scene: &Scene, x: u32, y: u32) -> Option<String> {
    // Gate: outside the scene's outermost rect → no hit. Reuses
    // `Scene::hit_test` so the gate logic stays in lockstep with the
    // canonical pinion-core hit semantics (rect_contains gate +
    // Effect skip + zero-area rejection are all centralised there).
    scene.hit_test(x, y)?;
    let mut current = scene;
    let mut current_path = scene_root_path_segment(scene);
    let mut deepest_tagged_path: Option<String> = if scene_tag(scene).is_some() {
        Some(current_path.clone())
    } else {
        None
    };
    loop {
        // Match the visual nesting: topmost (last-drawn) sibling wins
        // on overlap, exactly mirroring `Scene::hit_test`'s reverse
        // iteration. Compute per-type nth-of-type indices in a forward
        // pass first so the matched index carries the same
        // disambiguator the forward walker (`scene_to_tree_item`)
        // would emit — the load-bearing round-trip invariant.
        let Scene::Container(c) = current else {
            // Non-Container variants (Text / Box / Path / Image /
            // External / Scroll / Effect) are leaves for the descent
            // — no children to walk into. The current
            // `deepest_tagged_path` is the answer.
            //
            // For `Scene::Scroll`, this stops the walk at the Scroll
            // node itself rather than descending into `content` with
            // offset translation; documented as the v1 carry above.
            break;
        };
        let mut type_counts: HashMap<&'static str, usize> = HashMap::new();
        let mut child_path_segments: Vec<String> = Vec::with_capacity(c.children.len());
        for child in &c.children {
            let ty = scene_type_name(child);
            let nth_ref = type_counts.entry(ty).or_insert(0);
            let nth = *nth_ref;
            *nth_ref += 1;
            child_path_segments.push(scene_child_path_segment(child, nth));
        }
        let hit_idx = c
            .children
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, child)| child.hit_test(x, y).map(|_| i));
        let Some(idx) = hit_idx else {
            // Container itself is the deepest hit — no descendant
            // contains the point. Walk terminates here.
            break;
        };
        current_path.push('/');
        current_path.push_str(&child_path_segments[idx]);
        let child_scene = &c.children[idx];
        if scene_tag(child_scene).is_some() {
            deepest_tagged_path = Some(current_path.clone());
        }
        current = child_scene;
    }
    deepest_tagged_path
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

/// R683.C §5.16 §5.20 §5.49 — bare event-name the [`ClickRouter`] emits
/// on each accepted invoke. The runtime intent-queue walker prefixes
/// this with the producing External's tag, so the binding-side reducer
/// matches the dotted form `{tag}.click` per
/// [[intent-tag-dotted-wire-form]]. Public so consumers can spell the
/// dotted tag via [`intent_tag!`](pinion_core::intent_tag) when the
/// macro grows non-literal support — pre that, bindings literal the
/// concatenation themselves (`intent_tag!("my_router", "click")`).
pub const CLICK_ROUTER_EVENT: &str = "click";
/// R683.C §5.16 §5.49 — `ExternalIntrospect` schema slot name for the
/// read-only "most-recent-invoke-payload" mirror. AI clients
/// `scene/query {path: "/{router_tag}/external/last_clicked"}` to
/// observe the latest accepted invoke without firing a fresh event.
pub const CLICK_ROUTER_LAST_CLICKED_SLOT: &str = "last_clicked";
/// R683.C §5.16 §5.49 — `ExternalIntrospect` invoke path the channel
/// is bound to. `scene/invoke {path: "/{router_tag}/external/click",
/// args: Text(path)}` selects; `args: Null` deselects.
pub const CLICK_ROUTER_INVOKE_PATH: &str = "click";

/// R683.C §5.16 §5.20 §5.49 — backend-agnostic AI-driven click-routing
/// `External`. Bridges a typed `scene/invoke` call into a `click`
/// intent the binding's [`WidgetCore::update`](pinion_core::WidgetCore::update)
/// reducer can mirror into a reactive selection slot (canonically a
/// shared `Signal<Option<String>>` resolved through
/// [`Owner::cache`](pinion_core::Owner::cache)).
///
/// Lifted at R683.C from `examples/hello-multi-window`'s
/// `MainWindowClickRouter` (1st consumer, R679) after
/// `examples/hello-dock-panels` (2nd consumer, R683.C) reached the
/// `Rule-of-Three` trigger per
/// [[abstraction-needs-second-consumer]]. The 2nd consumer's
/// requirements are bit-identical: AI-invoke-only `click` channel
/// (`Text(path)` selects, `Null` deselects), read-only `last_clicked`
/// mirror slot for [[ai-first-rpc-introspection-obligation]]-style
/// observation, single intent emit per accepted invoke (no no-op
/// fallthrough).
///
/// ## Wire
///
/// Binding registers an instance via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
/// tagged with the binding-local router tag (e.g.
/// `"main_click_router"`, `"viewport_click_router"`). RPC clients
/// drive it via `scene/invoke /{tag}/external/click`; the substrate
/// records the payload into `last_clicked` + enqueues a `click`
/// intent the framework's per-frame drain ships into the binding's
/// reducer prefixed with the router's tag (the dotted form
/// `{tag}.click` consumers match against).
///
/// ## Why an `External` (vs. a plain RPC method)
///
/// The bridge is paint-side reactive: the binding's reducer mutates a
/// `Signal<Option<String>>` whose change re-runs every subscribed
/// view fn (main paint with the selection wrap; inspector paint with
/// the focus state-layer; both observe the same `Owner::cache` slot).
/// A bare RPC method would short-circuit the reactive arc — the
/// External keeps the canonical Signal→view→paint→snapshot chain
/// intact so the AI client's `scene/snapshot` post-invoke observes the
/// same scene the user would see post-mouse-click.
///
/// ## Versus the [`TreeRowClickExternal`](crate::tree_view::TreeRowClickExternal)
///
/// `TreeRowClickExternal` is a real paint-side `External` with an
/// SCXML statechart (`Idle ↔ Pressed`) that user mouse clicks on a
/// tagged row drive via the `InputRouter`'s composite-tag dispatch.
/// `ClickRouter` is the inverse: there is **no SCXML**, **no paint-
/// side mouse handling** — only the AI-driven `invoke("click", ...)`
/// channel. User-mouse-driven clicks travel through whatever
/// paint-side widget the binding wires up (a `Button`, a
/// `TreeRowClickExternal`, …) and the binding's reducer mirrors that
/// arc's `click` intent into the same Signal that AI invokes write.
/// The two `External`s are complementary halves of the bidirectional
/// select arc — neither subsumes the other.
#[derive(Debug, Default)]
pub struct ClickRouter {
    /// Mirror of the last accepted `invoke("click", _)` arg.
    /// `Some(path)` after a `Text(path)` select invoke, `None` after
    /// a `Null` deselect invoke or before the first invoke. Returned
    /// verbatim through the [`CLICK_ROUTER_LAST_CLICKED_SLOT`] query
    /// channel.
    last_clicked: Option<String>,
    /// §5.20 intent buffer. [`External::drain_intents`] ships each
    /// queued [`Intent`] across the boundary on the next substrate
    /// drain pass. v1 enqueues exactly one intent per accepted
    /// invoke; the `Vec` shape leaves room for future multi-event
    /// invokes without breaking the queue contract.
    pending: Vec<Intent>,
}

impl ClickRouter {
    /// (R683.C §5.16 §5.49) Construct a fresh router with no recorded
    /// click and an empty intent buffer. Substrate calls this once at
    /// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
    /// time per registered router instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only accessor for the [`CLICK_ROUTER_LAST_CLICKED_SLOT`]
    /// mirror. Returns `None` when no `invoke` has fired or when the
    /// last invoke was a `Null` deselect. Mirrors the query channel
    /// the introspect surface exposes — useful for unit tests that
    /// pin the substrate's behaviour without spinning up the full
    /// `ExternalIntrospect` dispatch.
    #[must_use]
    pub fn last_clicked(&self) -> Option<&str> {
        self.last_clicked.as_deref()
    }
}

impl External for ClickRouter {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending.drain(..) {
            sink(intent);
        }
    }

    fn is_dirty(&self) -> bool {
        !self.pending.is_empty()
    }
}

impl ExternalIntrospect for ClickRouter {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new(CLICK_ROUTER_LAST_CLICKED_SLOT, "string"),
                    // R1643 — an ACTION, which is what it has always been.
                    //
                    // Declared with `new` since R683.C, so `$schema` said "query
                    // me" about the one name only `invoke` answers — and `query`
                    // returns `None` for it, so the declaration was wrong in
                    // both directions at once. Invisible until R1637 made the
                    // declaration a precondition of dispatch, at which point
                    // three demos began refusing with `PathIsAReadSlot`
                    // (`r679`, `r680`, `r683`) — and stayed red, because the
                    // catalog walk that would have caught it reaches
                    // `pinion-core` only and nothing in this crate's devtools
                    // module had a test at all
                    // ([[debt-the-declaration-walk-reaches-one-crate]], closed
                    // by widening that walk to this crate in the same round).
                    SchemaField::action(CLICK_ROUTER_INVOKE_PATH, "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        if path == CLICK_ROUTER_LAST_CLICKED_SLOT {
            return Ok(match &self.last_clicked {
                Some(p) => IntrospectValue::Text(p.clone()),
                None => IntrospectValue::Null,
            });
        }
        Err(ReadRefusal::UnknownPath)
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        if path == CLICK_ROUTER_LAST_CLICKED_SLOT {
            return Err(InterveneError::ReadOnly);
        }
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path != CLICK_ROUTER_INVOKE_PATH {
            return Err(InvokeError::UnknownPath);
        }
        match args {
            IntrospectValue::Text(p) => {
                self.last_clicked = Some(p.clone());
                self.pending.push(Intent::new_static(
                    CLICK_ROUTER_EVENT,
                    IntrospectValue::Text(p),
                ));
                Ok(IntrospectValue::Bool(true))
            }
            IntrospectValue::Null => {
                self.last_clicked = None;
                self.pending.push(Intent::new_static(
                    CLICK_ROUTER_EVENT,
                    IntrospectValue::Null,
                ));
                Ok(IntrospectValue::Bool(true))
            }
            _ => Err(InvokeError::TypeMismatch),
        }
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
    use pinion_core::Color;
    use pinion_core::scene::{ContainerNode, ExternalNode, Rect, TextNode};
    use pinion_core::style::TextStyle;

    fn tagged_container(tag: &'static str, children: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(children).with_tag(tag))
    }

    fn untagged_container(children: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(children))
    }

    fn text_node(content: &str) -> Scene {
        Scene::Text(TextNode::styled(content, Rect::default(), TextStyle::new()))
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
        let ext =
            Scene::External(ExternalNode::new(Box::new(NoopExternal)).with_tag("inspector_tree"));
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

    // ---------- path_for_paint_hit ----------
    //
    // R679 §5.16 §5.49 — paint-side hit-test inverse walker. The
    // load-bearing invariant: every successful hit returns a path
    // that `find_node_at_path` resolves on the same scene
    // (round-trip). The walker tracks the *deepest tagged ancestor*
    // along the hit chain so untagged decoration (label Text inside
    // a tagged button, flex wrappers) is walked past — mirroring
    // the runtime `InputRouter::resolve_hover_tag` semantics so the
    // (x, y) → path map is consistent across every paint-side input
    // arc.

    use pinion_core::scene::{BoxNode, ScrollNode};

    /// Build a `Scene::Box` with a concrete rect — Box variants
    /// carry their own rect inline so hit-test grids resolve
    /// without a layout pass.
    fn tagged_box(tag: &'static str, x: u32, y: u32, w: u32, h: u32) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()).with_tag(tag))
    }

    fn untagged_box(x: u32, y: u32, w: u32, h: u32) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()))
    }

    /// Wrap children in a Container with an explicitly assigned
    /// rect. `ContainerNode::new()` leaves `rect = Rect::default()`
    /// (zero); the layout pass at paint time normally fills it.
    /// Substrate tests run **without** the layout pass — locate.rs
    /// uses the same `node.rect = ...` direct assignment pattern.
    fn tagged_container_with_children(
        tag: &'static str,
        rect: Rect,
        children: Vec<Scene>,
    ) -> Scene {
        let mut node = ContainerNode::new(children).with_tag(tag);
        node.rect = rect;
        Scene::Container(node)
    }

    fn untagged_container_with_children(rect: Rect, children: Vec<Scene>) -> Scene {
        let mut node = ContainerNode::new(children);
        node.rect = rect;
        Scene::Container(node)
    }

    #[test]
    fn r679_path_for_paint_hit_outside_scene_returns_none() {
        // No tagged ancestor anywhere on a hit OUTSIDE the scene →
        // None.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![tagged_box("btn", 10, 10, 40, 40)],
        );
        // Point at (500, 500) is outside the container → no hit.
        assert!(path_for_paint_hit(&scene, 500, 500).is_none());
    }

    #[test]
    fn r679_path_for_paint_hit_on_untagged_only_scene_returns_none() {
        // Hit lands inside an untagged Box inside an untagged
        // Container — no tagged ancestor along the chain.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![untagged_box(0, 0, 50, 50)],
        );
        // (10, 10) lands on the untagged box.
        assert!(path_for_paint_hit(&scene, 10, 10).is_none());
    }

    #[test]
    fn r679_path_for_paint_hit_on_tagged_container_returns_container_path() {
        // Root untagged Container holds a tagged Container holding
        // geometry. Hit on the tagged Container → path =
        // "Container/Container[btn]".
        //
        // Note: only Container + External are tag-bearing in the
        // devtools path scheme (per R678 docs on `scene_tag`).
        // Tagged Box/Text/etc. are NOT recognised as
        // deepest-tagged-ancestor candidates here; the per-type
        // nth-of-type form is used instead.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![tagged_container_with_children(
                "btn",
                Rect::new(0, 0, 40, 40),
                vec![untagged_box(0, 0, 40, 40)],
            )],
        );
        let path = path_for_paint_hit(&scene, 10, 10).expect("must hit tagged container");
        assert_eq!(path, "Container/Container[btn]");
        // Round-trip invariant.
        assert!(
            find_node_at_path(&scene, &path).is_some(),
            "path_for_paint_hit output must resolve via find_node_at_path",
        );
    }

    #[test]
    fn r679_path_for_paint_hit_tagged_box_not_recognised_as_tagged_ancestor() {
        // Per R678 `scene_tag` contract, only Container + External
        // are tag-bearing. A tagged Box at the top of the hit
        // chain does not contribute a tagged ancestor — the
        // walker walks past it (treating it as an anonymous
        // sibling) and looks for tagged Container/External
        // ancestors instead. If there are none, returns None.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![tagged_box("box_tag_ignored", 0, 0, 40, 40)],
        );
        assert!(
            path_for_paint_hit(&scene, 10, 10).is_none(),
            "tagged Box is not a devtools-path-scheme tag",
        );
    }

    #[test]
    fn r679_path_for_paint_hit_walks_past_untagged_leaf_to_tagged_ancestor() {
        // Tagged Container wraps a tagged Box (inner). Hit on the
        // box → returns the inner Box path; if we replace the Box
        // with an untagged Text, the walker should walk back to
        // the outer tagged Container. We pin both flavours below.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![tagged_container_with_children(
                "btn",
                Rect::new(0, 0, 50, 50),
                vec![Scene::Text(TextNode::styled(
                    "Click me!",
                    Rect::new(5, 5, 30, 20),
                    TextStyle::new(),
                ))],
            )],
        );
        // Hit on (10, 10): the Text rect contains it. Text is
        // untagged so the deepest tagged is the parent Container.
        let path = path_for_paint_hit(&scene, 10, 10).expect("must hit");
        assert_eq!(path, "Container/Container[btn]");
        assert!(find_node_at_path(&scene, &path).is_some());
    }

    #[test]
    fn r679_path_for_paint_hit_returns_deepest_tagged_when_nested() {
        // Two nested tagged Containers — hit on the inner one
        // returns the inner path (deepest tagged ancestor closer
        // to the leaf wins).
        let scene = tagged_container_with_children(
            "outer",
            Rect::new(0, 0, 100, 100),
            vec![tagged_container_with_children(
                "inner",
                Rect::new(0, 0, 50, 50),
                vec![untagged_box(0, 0, 30, 30)],
            )],
        );
        let path = path_for_paint_hit(&scene, 10, 10).expect("must hit");
        assert_eq!(
            path, "Container[outer]/Container[inner]",
            "deepest tagged ancestor at hit point wins",
        );
        assert!(find_node_at_path(&scene, &path).is_some());
    }

    #[test]
    fn r679_path_for_paint_hit_overlapping_siblings_topmost_wins() {
        // Two tagged Containers overlap. Reverse-iteration in
        // `path_for_paint_hit` matches `Scene::hit_test` —
        // declaration-order-last sibling drawn on top wins.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![
                tagged_container_with_children(
                    "lower",
                    Rect::new(0, 0, 50, 50),
                    vec![untagged_box(0, 0, 50, 50)],
                ),
                tagged_container_with_children(
                    "upper",
                    Rect::new(10, 10, 50, 50),
                    vec![untagged_box(10, 10, 50, 50)],
                ),
            ],
        );
        // (20, 20) is inside both; topmost (upper, declared 2nd)
        // wins.
        let path = path_for_paint_hit(&scene, 20, 20).expect("must hit");
        assert_eq!(path, "Container/Container[upper]");
        assert!(find_node_at_path(&scene, &path).is_some());
    }

    #[test]
    fn r679_path_for_paint_hit_per_type_idx_among_untagged_siblings() {
        // Three untagged Boxes inside a tagged Container —
        // per-type idx for non-tagged children matches the forward
        // walker's `Type[nth-of-type]` form. Hit on the 2nd box
        // walks back to the tagged parent (the deepest tagged
        // ancestor) — the untagged box itself has no tag so it
        // does not contribute a tagged path.
        let scene = tagged_container_with_children(
            "root",
            Rect::new(0, 0, 100, 100),
            vec![
                untagged_box(0, 0, 20, 20),
                untagged_box(30, 0, 20, 20),
                untagged_box(60, 0, 20, 20),
            ],
        );
        // Hit on the 2nd untagged box (declaration idx 1) → no
        // tagged descendant → deepest tagged is the root.
        let path = path_for_paint_hit(&scene, 35, 5).expect("must hit");
        assert_eq!(path, "Container[root]");
        assert!(find_node_at_path(&scene, &path).is_some());
    }

    #[test]
    fn r679_path_for_paint_hit_mixed_type_siblings_use_per_type_idx() {
        // Tagged Container holds a Box, a tagged inner Container,
        // and another Box. The inner tagged Container's path uses
        // its tag form (priority over nth-of-type).
        let scene = tagged_container_with_children(
            "root",
            Rect::new(0, 0, 100, 100),
            vec![
                untagged_box(0, 0, 20, 20),
                tagged_container_with_children(
                    "mid",
                    Rect::new(20, 20, 30, 30),
                    vec![untagged_box(20, 20, 20, 20)],
                ),
                untagged_box(60, 0, 20, 20),
            ],
        );
        // Hit (25, 25) lands on the inner box inside `mid`. The
        // deepest tagged is `mid`.
        let path = path_for_paint_hit(&scene, 25, 25).expect("must hit");
        assert_eq!(path, "Container[root]/Container[mid]");
        assert!(find_node_at_path(&scene, &path).is_some());
    }

    #[test]
    fn r679_path_for_paint_hit_root_tag_alone_when_root_hit() {
        // Tagged root Container with an untagged Text child. Hit
        // inside the Text → walks back to root (deepest tagged
        // ancestor = root itself).
        let scene = tagged_container_with_children(
            "root",
            Rect::new(0, 0, 50, 50),
            vec![Scene::Text(TextNode::styled(
                "x",
                Rect::new(0, 0, 10, 10),
                TextStyle::new(),
            ))],
        );
        let path = path_for_paint_hit(&scene, 0, 0).expect("must hit");
        assert_eq!(path, "Container[root]");
        assert!(find_node_at_path(&scene, &path).is_some());
    }

    #[test]
    fn r679_path_for_paint_hit_scroll_does_not_descend_into_content_v1_carry() {
        // V1 carry: `Scene::Scroll` walks down to the Scroll node
        // itself but does not descend into `content` with offset
        // translation. A Scroll inside an untagged tree returns
        // None even when the content holds a tagged descendant.
        // Documented as the substrate carry — R680+ DevTools-on-
        // Scroll consumer will land the offset-translation
        // descent matching `Scene::hit_test`.
        let scene = untagged_container_with_children(
            Rect::new(0, 0, 100, 100),
            vec![Scene::Scroll(ScrollNode::new(
                Rect::new(0, 0, 100, 100),
                tagged_box("inside_scroll", 0, 0, 50, 50),
            ))],
        );
        // Hit inside the scroll viewport (0,0)..(100,100). Content
        // holds a tagged Box but the walker doesn't descend.
        let result = path_for_paint_hit(&scene, 10, 10);
        // Scroll content's tag is not reached; no tagged ancestor
        // above the Scroll either → None.
        assert!(
            result.is_none(),
            "v1 carry: Scroll content descent not yet implemented",
        );
    }

    #[test]
    fn r679_path_for_paint_hit_round_trip_for_every_position_in_tagged_grid() {
        // Stress test: a 3x3 grid of tagged Containers (9 tagged
        // children). For every (x, y) inside the grid, the
        // returned path must round-trip through find_node_at_path.
        let mut children: Vec<Scene> = Vec::with_capacity(9);
        for col in 0..3_u32 {
            for row in 0..3_u32 {
                let tag: &'static str = match (col, row) {
                    (0, 0) => "c00",
                    (1, 0) => "c10",
                    (2, 0) => "c20",
                    (0, 1) => "c01",
                    (1, 1) => "c11",
                    (2, 1) => "c21",
                    (0, 2) => "c02",
                    (1, 2) => "c12",
                    (2, 2) => "c22",
                    _ => unreachable!(),
                };
                children.push(tagged_container_with_children(
                    tag,
                    Rect::new(col * 20, row * 20, 20, 20),
                    vec![untagged_box(col * 20, row * 20, 20, 20)],
                ));
            }
        }
        let scene = untagged_container_with_children(Rect::new(0, 0, 60, 60), children);
        for x in [5_u32, 25, 45] {
            for y in [5_u32, 25, 45] {
                let path = path_for_paint_hit(&scene, x, y)
                    .unwrap_or_else(|| panic!("hit at ({x}, {y}) must resolve"));
                assert!(
                    find_node_at_path(&scene, &path).is_some(),
                    "round-trip failed at ({x}, {y}): path={path:?}",
                );
            }
        }
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

    // R683.C §5.16 §5.20 §5.49 — `ClickRouter` substrate regression
    // suite. Lifted at R683.C from the binding-side
    // `r679_main_window_click_router_*` tests in
    // `examples/hello-multi-window/src/main.rs`; same behaviours, now
    // pinned at the substrate layer so every future consumer
    // (hello-dock-panels = 2nd, future Inspector panes = 3rd, …)
    // inherits coverage.
    mod r683_click_router_tests {
        use super::super::{
            CLICK_ROUTER_EVENT, CLICK_ROUTER_INVOKE_PATH, CLICK_ROUTER_LAST_CLICKED_SLOT,
            ClickRouter,
        };
        use pinion_core::external::ReadRefusal;
        use pinion_core::external::{
            Backend, External, ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
        };

        #[test]
        fn r683_new_router_starts_with_no_recorded_click_and_empty_queue() {
            let router = ClickRouter::new();
            assert_eq!(router.last_clicked(), None);
            assert!(!router.is_dirty(), "fresh router holds no pending intents");
        }

        #[test]
        fn r683_router_backends_cover_gui_tui_rpc() {
            let router = ClickRouter::new();
            let support = router.backends();
            // GUI + TUI + RPC must all be supported so the substrate
            // works under every pinion shell + the headless RPC
            // backend.
            assert!(support.supports(Backend::Gui));
            assert!(support.supports(Backend::Tui));
            assert!(support.supports(Backend::Rpc));
        }

        #[test]
        fn r683_router_schema_exposes_canonical_slot_names() {
            let router = ClickRouter::new();
            let schema = router.schema();
            let names: Vec<&str> = schema.fields.iter().map(|f| f.path).collect();
            assert!(names.contains(&CLICK_ROUTER_LAST_CLICKED_SLOT));
            assert!(names.contains(&CLICK_ROUTER_INVOKE_PATH));
        }

        #[test]
        fn r683_query_last_clicked_returns_null_pre_invoke() {
            let router = ClickRouter::new();
            assert_eq!(
                router.query(CLICK_ROUTER_LAST_CLICKED_SLOT),
                Ok(IntrospectValue::Null),
            );
        }

        #[test]
        fn r683_query_unknown_path_returns_none() {
            let router = ClickRouter::new();
            assert_eq!(router.query("nonexistent"), Err(ReadRefusal::UnknownPath));
        }

        #[test]
        fn r683_invoke_click_with_text_records_path_and_enqueues_intent() {
            let mut router = ClickRouter::new();
            let res = router
                .invoke(
                    CLICK_ROUTER_INVOKE_PATH,
                    IntrospectValue::Text("foo/bar".into()),
                )
                .expect("invoke must succeed for Text payload");
            assert_eq!(res, IntrospectValue::Bool(true));
            assert_eq!(router.last_clicked(), Some("foo/bar"));
            assert!(router.is_dirty(), "invoke must enqueue an intent");

            let mut drained: Vec<pinion_core::intent::Intent> = Vec::new();
            router.drain_intents(&mut |intent| drained.push(intent));
            assert_eq!(drained.len(), 1);
            assert_eq!(drained[0].tag_str(), CLICK_ROUTER_EVENT);
            assert_eq!(drained[0].payload.as_str(), Some("foo/bar"));
        }

        #[test]
        fn r683_invoke_click_with_null_clears_path_and_enqueues_null_intent() {
            let mut router = ClickRouter::new();
            // Prime the mirror by selecting first.
            router
                .invoke(
                    CLICK_ROUTER_INVOKE_PATH,
                    IntrospectValue::Text("primed".into()),
                )
                .unwrap();
            // Drain so the next emit isolates the deselect intent.
            router.drain_intents(&mut |_| {});

            let res = router
                .invoke(CLICK_ROUTER_INVOKE_PATH, IntrospectValue::Null)
                .expect("invoke must succeed for Null payload");
            assert_eq!(res, IntrospectValue::Bool(true));
            assert_eq!(router.last_clicked(), None, "Null clears the mirror");

            let mut drained: Vec<pinion_core::intent::Intent> = Vec::new();
            router.drain_intents(&mut |intent| drained.push(intent));
            assert_eq!(drained.len(), 1);
            assert_eq!(drained[0].payload, IntrospectValue::Null);
        }

        #[test]
        fn r683_invoke_click_with_other_payload_returns_type_mismatch() {
            let mut router = ClickRouter::new();
            let res = router.invoke(CLICK_ROUTER_INVOKE_PATH, IntrospectValue::Bool(true));
            assert!(matches!(res, Err(InvokeError::TypeMismatch)));
            assert_eq!(
                router.last_clicked(),
                None,
                "rejected invoke must not mutate state"
            );
        }

        #[test]
        fn r683_invoke_unknown_path_returns_unknown_path_error() {
            let mut router = ClickRouter::new();
            let res = router.invoke("ghost", IntrospectValue::Text("ignored".into()));
            assert!(matches!(res, Err(InvokeError::UnknownPath)));
        }

        #[test]
        fn r683_intervene_last_clicked_is_read_only() {
            let mut router = ClickRouter::new();
            let res = router.intervene(
                CLICK_ROUTER_LAST_CLICKED_SLOT,
                IntrospectValue::Text("attempted".into()),
            );
            assert!(matches!(res, Err(InterveneError::ReadOnly)));
        }

        #[test]
        fn r683_intervene_unknown_path_returns_unknown_path_error() {
            let mut router = ClickRouter::new();
            let res = router.intervene("ghost", IntrospectValue::Null);
            assert!(matches!(res, Err(InterveneError::UnknownPath)));
        }

        #[test]
        fn r683_drain_intents_empties_the_pending_queue() {
            let mut router = ClickRouter::new();
            router
                .invoke(CLICK_ROUTER_INVOKE_PATH, IntrospectValue::Text("a".into()))
                .unwrap();
            assert!(router.is_dirty());
            router.drain_intents(&mut |_| {});
            assert!(!router.is_dirty(), "drain must empty the queue");
        }

        #[test]
        fn r683_multiple_invokes_record_latest_and_enqueue_each_intent() {
            let mut router = ClickRouter::new();
            router
                .invoke(CLICK_ROUTER_INVOKE_PATH, IntrospectValue::Text("a".into()))
                .unwrap();
            router
                .invoke(CLICK_ROUTER_INVOKE_PATH, IntrospectValue::Text("b".into()))
                .unwrap();
            assert_eq!(router.last_clicked(), Some("b"));

            let mut drained: Vec<pinion_core::intent::Intent> = Vec::new();
            router.drain_intents(&mut |intent| drained.push(intent));
            assert_eq!(drained.len(), 2);
            assert_eq!(drained[0].payload.as_str(), Some("a"));
            assert_eq!(drained[1].payload.as_str(), Some("b"));
        }

        #[test]
        fn r683_introspect_query_reflects_recorded_path() {
            let mut router = ClickRouter::new();
            router
                .invoke(
                    CLICK_ROUTER_INVOKE_PATH,
                    IntrospectValue::Text("p/q".into()),
                )
                .unwrap();
            assert_eq!(
                router.query(CLICK_ROUTER_LAST_CLICKED_SLOT),
                Ok(IntrospectValue::Text("p/q".into())),
            );
        }
    }
}
