//! R51.69 §5.40 — accessible-name derivation from the paint scene.
//!
//! WAI-ARIA 1.2 §4.3 "Accessible Name and Description Computation"
//! prescribes a precedence chain: explicit `aria-label` ≻ associated
//! `aria-labelledby` ≻ host-language label ≻ name from content
//! (button text, link text). Pinion mirrors the most common two
//! rungs:
//!
//! 1. `ContainerNode::aria_label` override (the `aria-label`
//!    analogue), set declaratively on the widget's outer tagged
//!    container in the view-fn.
//! 2. First-descendant [`TextNode`] content (the "name from content"
//!    rung — what `<button>Save</button>` does in HTML).
//!
//! The widget's `access_node` impl returns its semantic descriptor
//! (role / state / value) only; the accessible-name field is filled
//! in by [`enrich_names_from_scene`] after layout, walking the same
//! paint scene the renderer consumes. Doing the derivation here
//! keeps a single source of truth — the label literal lives once,
//! in the `view` function — and removes the duplicate match blocks
//! that earlier `access_node` impls carried.
//!
//! `aria-labelledby` (cross-node label association) is carry — it
//! requires per-node IDs in the scene graph beyond `tag`, which the
//! current §5.20 intent-system reuses.

use pinion_core::Scene;

use crate::AccessNode;

/// Populate `nodes[*].name` from the paint scene.
///
/// For each [`AccessNode`] whose `name` is still `None`, walks `scene`
/// looking for the [`Scene::Container`] whose `tag` matches the node's
/// `tag`. When found, applies the WAI-ARIA name-computation precedence:
///
/// 1. If the matched container has [`ContainerNode::aria_label`]
///    (`Some(…)`), use it — that mirrors `<button aria-label="…">`.
/// 2. Otherwise, find the first descendant [`TextNode`] in DFS
///    pre-order and use its `content` — that mirrors "name from
///    contents" for `<button>Click me!</button>`.
/// 3. Otherwise, leave `name` as `None` — AT clients then receive an
///    unnamed but still discoverable node.
///
/// The derivation is a no-op for any node whose `name` is already
/// `Some(_)` (an explicit `with_name` call on the widget side wins
/// over scene-derived names, preserving the override path for
/// widgets that fundamentally cannot expose their label in a Text
/// leaf, e.g. icon-only controls without an `aria_label` modifier
/// — those should set `with_name` directly).
///
/// `ContainerNode` matching is tag-exact (`==`). Composite child
/// tags carry a `#` separator (`"radio_group#0"`); the enrichment
/// resolves only the parent tag's container — composite child name
/// derivation is part of R51.70 child-dispatch carry.
///
/// Returns the number of nodes whose `name` field was populated.
/// The count helps the conformance test verify the enrichment
/// actually ran for the expected widget population.
pub fn enrich_names_from_scene(nodes: &mut [AccessNode], scene: &Scene) -> usize {
    let mut filled = 0usize;
    for node in nodes.iter_mut() {
        if node.name.is_some() {
            continue;
        }
        let Some(container) = find_container_by_tag(scene, &node.tag) else {
            continue;
        };
        if let Some(label) = container.aria_label.as_deref() {
            node.name = Some(first_line(label).to_string());
            filled += 1;
            continue;
        }
        if let Some(text) = walk_for_text(container) {
            node.name = Some(first_line(&text).to_string());
            filled += 1;
        }
    }
    filled
}

/// Walk `scene` looking for the first [`Scene::Container`] whose `tag`
/// equals `target`. Returns a borrow so the caller reads `aria_label`
/// and children without cloning the subtree.
fn find_container_by_tag<'s>(
    scene: &'s Scene,
    target: &str,
) -> Option<&'s pinion_core::scene::ContainerNode> {
    match scene {
        Scene::Container(c) => {
            if c.tag.as_deref() == Some(target) {
                return Some(c);
            }
            for child in &c.children {
                if let Some(hit) = find_container_by_tag(child, target) {
                    return Some(hit);
                }
            }
            None
        }
        _ => None,
    }
}

/// DFS pre-order over `scene`, returning the `content` of the first
/// [`Scene::Text`] reached. Skips [`Scene::Effect`] (no geometry) and
/// stops at the first hit. Returns `None` for scenes with no text.
fn first_text_leaf(scene: &Scene) -> Option<String> {
    match scene {
        Scene::Text(t) => Some(t.content.clone()),
        Scene::Container(c) => c.children.iter().find_map(first_text_leaf),
        _ => None,
    }
}

/// Trim to the substring before the first `'\n'`. Mirrors AT-SPI /
/// UIA expectations that an accessible name is a single line — multi-
/// line labels collapse to the first line, full description belongs
/// in `aria-description` (carry).
fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or(s)
}

/// Re-entry helper that walks the children slice of a borrowed
/// container without forcing it through `Scene::Container`.
/// [`ContainerNode`] does not implement `Clone` (its `ExternalNode`
/// children carry a `Box<dyn External>` with no generic clone
/// strategy per scene.rs), so [`first_text_leaf`] cannot be reached
/// directly from a `&ContainerNode` borrow — this walks the
/// children slice in DFS pre-order and stops at the first match.
fn walk_for_text(container: &pinion_core::scene::ContainerNode) -> Option<String> {
    for child in &container.children {
        if let Some(found) = first_text_leaf(child) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AriaRole};
    use pinion_core::scene::{ContainerNode, Rect, TextNode};

    fn button_scene_with_text(tag: &'static str, label: &'static str) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                label.to_string(),
                Rect::default(),
            ))])
            .with_tag(tag),
        )
    }

    #[test]
    fn fills_name_from_first_text_leaf() {
        let scene = button_scene_with_text("main_btn", "Click me!");
        let mut nodes = vec![AccessNode::new("main_btn", AriaRole::Button)];
        let filled = enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(filled, 1);
        assert_eq!(nodes[0].name.as_deref(), Some("Click me!"));
    }

    #[test]
    fn aria_label_override_wins_over_text() {
        let scene = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "Visible".to_string(),
                Rect::default(),
            ))])
            .with_tag("main_btn")
            .with_aria_label("Save document"),
        );
        let mut nodes = vec![AccessNode::new("main_btn", AriaRole::Button)];
        enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("Save document"));
    }

    #[test]
    fn existing_name_is_preserved() {
        let scene = button_scene_with_text("main_btn", "Click me!");
        let mut nodes = vec![
            AccessNode::new("main_btn", AriaRole::Button).with_name("Explicit")
        ];
        let filled = enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(filled, 0);
        assert_eq!(nodes[0].name.as_deref(), Some("Explicit"));
    }

    #[test]
    fn unknown_tag_leaves_name_none() {
        let scene = button_scene_with_text("other", "Hi");
        let mut nodes = vec![AccessNode::new("main_btn", AriaRole::Button)];
        let filled = enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(filled, 0);
        assert!(nodes[0].name.is_none());
    }

    #[test]
    fn first_line_collapses_multiline() {
        let scene = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "First\nSecond".to_string(),
                Rect::default(),
            ))])
            .with_tag("t"),
        );
        let mut nodes = vec![AccessNode::new("t", AriaRole::Button)];
        enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("First"));
    }

    #[test]
    fn nested_container_text_resolves() {
        let inner = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "Inner".to_string(),
                Rect::default(),
            ))])
            .with_tag("inner"),
        );
        let outer = Scene::Container(ContainerNode::new(vec![inner]).with_tag("outer"));
        let mut nodes = vec![
            AccessNode::new("outer", AriaRole::Button),
            AccessNode::new("inner", AriaRole::Button),
        ];
        let filled = enrich_names_from_scene(&mut nodes, &outer);
        assert_eq!(filled, 2);
        assert_eq!(nodes[0].name.as_deref(), Some("Inner"));
        assert_eq!(nodes[1].name.as_deref(), Some("Inner"));
    }

    #[test]
    fn empty_container_yields_no_name() {
        let scene = Scene::Container(ContainerNode::new(vec![]).with_tag("empty"));
        let mut nodes = vec![AccessNode::new("empty", AriaRole::Button)];
        let filled = enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(filled, 0);
        assert!(nodes[0].name.is_none());
    }

    #[test]
    fn walk_for_text_visits_in_order() {
        let container = ContainerNode::new(vec![
            Scene::Container(ContainerNode::new(vec![])),
            Scene::Text(TextNode::new("First".to_string(), Rect::default())),
            Scene::Text(TextNode::new("Second".to_string(), Rect::default())),
        ]);
        assert_eq!(walk_for_text(&container).as_deref(), Some("First"));
    }
}
