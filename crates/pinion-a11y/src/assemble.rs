//! R984 §5.40 §2 #7 — the backend-agnostic accessibility-tree assembler.
//!
//! [`build_access_tree`] is the SSOT every embedder runs to turn a binding's
//! [`WidgetA11y`](crate::WidgetA11y) projection into the enriched node list a
//! screen reader (the live `AccessKit` emit) and an AI client (the
//! `scene/access` RPC dump) both consume — the two MUST agree, so the assembly
//! lives here once rather than being re-derived in each shell. The GUI shell
//! (`pinion_shell`) was the first consumer (R979); the TUI shell (`pinion_tui`)
//! is the second (R984), so the shared steps lift here at the 2nd consumer
//! (divergence-is-a-bug) instead of being copied per backend.
//!
//! Two steps are backend-agnostic and live here:
//!   1. run the binding's node + focus projection in the reactive owner scope,
//!      enriching accessible names from the paint scene ([`build_access_tree`]);
//!   2. resolve each node's pixel [`bounds`](AccessNode::bounds) from a
//!      caller-supplied tag -> rect resolver ([`resolve_access_bounds`]).
//!
//! The rect resolver is a closure (not a hard call into a layout engine)
//! because the lookup (`pinion_runtime::rect_for_tag`) lives in a sibling crate
//! this one does not depend on — each shell passes its own, keeping the layering
//! acyclic while sharing the union-bounds policy.

use pinion_core::scene::Rect;
use pinion_core::{Owner, Scene};

use crate::{AccessFocus, AccessNode, enrich_names_from_scene};

/// Assemble the enriched accessibility node list + AT focus target.
///
/// Runs `node_fn` (the binding's `access_node` / `access_node_for_window`) and
/// `focus_fn` (its `access_focus_target`) inside `owner`'s reactive scope, then
/// fills each node's accessible name from `paint_scene` text where present
/// (a never-painted window passes `None`, leaving names unresolved). Pixel
/// bounds are a separate, backend-specific step — see [`resolve_access_bounds`]
/// — because the rect lookup lives above this crate in the layering.
///
/// The two closures (rather than a `WidgetA11y` bound) let a multi-window GUI
/// shell pass `access_node_for_window` and a single-window TUI shell pass
/// `access_node` through the same assembler.
pub fn build_access_tree(
    owner: &Owner,
    paint_scene: Option<&Scene>,
    node_fn: impl FnOnce() -> Vec<AccessNode>,
    focus_fn: impl FnOnce() -> Option<AccessFocus>,
) -> (Vec<AccessNode>, Option<AccessFocus>) {
    let mut nodes = owner.run(node_fn);
    if let Some(paint) = paint_scene {
        enrich_names_from_scene(&mut nodes, paint);
    }
    let focus = owner.run(focus_fn);
    (nodes, focus)
}

/// Resolve each node's pixel [`bounds`](AccessNode::bounds) from `resolver`, a
/// tag -> rect lookup the caller supplies (the GUI / TUI shells pass
/// `|tag| pinion_runtime::rect_for_tag(paint, tag)`). A node carrying
/// [`bounds_union_tags`](AccessNode::bounds_union_tags) takes the union of its
/// own rect with each fragment's (the frozen-split / tree-grid multi-fragment
/// row, R863). A tag the resolver cannot place leaves `bounds` `None`, the
/// "never painted" honest answer.
pub fn resolve_access_bounds(nodes: &mut [AccessNode], resolver: impl Fn(&str) -> Option<Rect>) {
    for node in nodes {
        let mut bounds = resolver(&node.tag);
        for extra in &node.bounds_union_tags {
            if let Some(rect) = resolver(extra) {
                bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
            }
        }
        node.bounds = bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::{build_access_tree, resolve_access_bounds};
    use crate::{AccessNode, AriaRole};
    use pinion_core::Owner;
    use pinion_core::scene::Rect;

    #[test]
    fn build_runs_closures_and_returns_nodes_and_focus() {
        let owner = Owner::new();
        let (nodes, focus) = build_access_tree(
            &owner,
            None,
            || vec![AccessNode::new("a", AriaRole::Button).with_name("Save")],
            || None,
        );
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "a");
        assert_eq!(nodes[0].name.as_deref(), Some("Save"));
        assert!(focus.is_none());
    }

    #[test]
    fn build_enriches_an_unnamed_node_from_the_paint_scene() {
        // R984.1 — covers `build_access_tree`'s enrich branch (the prior test
        // passed `None` paint, so name enrichment was never exercised — the H1
        // gap on the shared SSOT every backend, including the TUI, runs).
        use pinion_core::Scene;
        use pinion_core::scene::{ContainerNode, Rect, TextNode};
        let paint = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "Save",
                Rect::new(0, 0, 40, 16),
            ))])
            .with_tag("btn".to_owned()),
        );
        let owner = Owner::new();
        let (nodes, focus) = build_access_tree(
            &owner,
            Some(&paint),
            || vec![AccessNode::new("btn", AriaRole::Button)],
            || None,
        );
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Save"),
            "an unnamed node's name is enriched from the paint scene's text",
        );
        assert!(focus.is_none());
    }

    #[test]
    fn resolve_fills_bounds_from_the_resolver_and_unions_fragments() {
        let mut nodes = vec![
            AccessNode::new("solo", AriaRole::Button),
            AccessNode::new("row", AriaRole::Row).with_bounds_union_tag("frag"),
            AccessNode::new("absent", AriaRole::Button),
        ];
        let rects = |tag: &str| match tag {
            "solo" => Some(Rect::new(0, 0, 10, 4)),
            "row" => Some(Rect::new(0, 0, 8, 4)),
            "frag" => Some(Rect::new(8, 0, 8, 4)),
            _ => None,
        };
        resolve_access_bounds(&mut nodes, rects);
        assert_eq!(
            nodes[0].bounds,
            Some(Rect::new(0, 0, 10, 4)),
            "solo node takes its own rect"
        );
        assert_eq!(
            nodes[1].bounds,
            Some(Rect::new(0, 0, 16, 4)),
            "a multi-fragment row unions its own rect with the fragment's",
        );
        assert_eq!(
            nodes[2].bounds, None,
            "a tag the resolver cannot place stays unresolved"
        );
    }
}
