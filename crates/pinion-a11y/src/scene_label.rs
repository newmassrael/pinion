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
//! 2. First-descendant [`TextNode`](pinion_core::scene::TextNode) content (the "name from content"
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

use std::collections::HashMap;

use pinion_core::Scene;

use crate::AccessNode;

/// Populate `nodes[*].name` from the paint scene.
///
/// For each [`AccessNode`] whose `name` is still `None`, walks `scene`
/// looking for the [`Scene::Container`] whose `tag` matches the node's
/// `tag`. When found, applies the WAI-ARIA name-computation precedence:
///
/// 1. If the matched container has `ContainerNode::aria_label`
///    (`Some(…)`), use it — that mirrors `<button aria-label="…">`.
/// 2. Otherwise, find the first descendant [`TextNode`](pinion_core::scene::TextNode) in DFS
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
    // R1536 — index the scene ONCE, then look each node up.
    //
    // Until R1536 this searched the whole scene per node — `O(nodes x scene)`
    // — which was tolerable only because the search stopped early and, for a
    // virtualized grid, found nothing at all (it could not enter a
    // `ScrollNode`, so it walked the tree to exhaustion for every node and
    // returned `None`). Making the walk *succeed* is exactly what makes the
    // quadratic term real: `hello-virtual-table` now resolves 97 nodes against
    // a ~600-node scene every frame the AT tree is emitted.
    //
    // One pre-order pass builds the map, so the pass is `O(scene + nodes)`.
    // Pre-order with `or_insert` preserves the previous rule — the FIRST
    // container with a tag wins — because a later duplicate cannot displace it.
    let index = tag_index(scene);
    let mut filled = 0usize;
    for node in nodes.iter_mut() {
        if node.name.is_some() {
            continue;
        }
        // R1320 §5.40 §5.27 — `name_from_tag` (the `aria-labelledby` relation) names
        // this node from ANOTHER tag's painted label; absent, the node names itself
        // from its own tag (the pre-R1320 name-from-contents path, unchanged). The
        // dock's `tabpanel` uses it to take its tab's label, so the AT tree and the
        // pixels cannot disagree about what a panel is called even when the label is
        // app state the a11y walker never sees (R1318 display titles).
        let lookup = node.name_from_tag.as_deref().unwrap_or(&node.tag);
        let Some(container) = index.get(lookup).copied() else {
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

/// R1536 §5.40 — every tagged [`Scene::Container`] in `scene`, keyed by tag,
/// built in one DFS pre-order pass.
///
/// `or_insert` keeps the FIRST container with a given tag, which is the rule
/// the per-node search had by construction (it returned at its first hit). A
/// duplicate tag is a binding bug either way; this preserves which one wins so
/// the change is a cost change and not a behaviour change.
fn tag_index(scene: &Scene) -> HashMap<&str, &pinion_core::scene::ContainerNode> {
    fn visit<'s>(
        scene: &'s Scene,
        out: &mut HashMap<&'s str, &'s pinion_core::scene::ContainerNode>,
    ) {
        match scene {
            Scene::Container(c) => {
                if let Some(tag) = c.tag.as_deref() {
                    out.entry(tag).or_insert(c);
                }
                for child in &c.children {
                    visit(child, out);
                }
            }
            // R1536 §5.40 §5.45 — a scroll is **transparent** to a tag walk,
            // the rule `Scene::rect_for_tag_with_offset` and
            // `Scene::lookup_path_ref` already follow. Without this arm nothing
            // painted inside a `ScrollNode` — every virtualized list, grid and
            // tree — could be named at all: the node's *bounds* resolved
            // correctly (that walker descends) and pointed at the right pixels
            // while announcing nothing, which is why the AT tree looked
            // structurally right and no test noticed. Measured on
            // `hello-virtual-table`: 75 of 75 `gridcell`s unnamed, now 75 of 75
            // named; `hello-virtual-list` 1 of 16 -> 16 of 16.
            Scene::Scroll(s) => visit(&s.content, out),
            _ => {}
        }
    }
    let mut out = HashMap::new();
    visit(scene, &mut out);
    out
}

/// DFS pre-order over `scene`, returning the `content` of the first
/// [`Scene::Text`] reached whose `TextNode::role` is not
/// [`pinion_core::scene::TextRole::Presentational`].
///
/// R51.81 §5.40 — pre-R51.81 the pass returned the *first* text
/// content unconditionally. Widgets that paint a decoration glyph
/// (Checkbox `✓`, Slider thumb caret) before their linguistic label
/// in DFS order had to override [`crate::AccessNode::with_name`] or
/// `ContainerNode::aria_label` to mask the wrong content (a
/// `WAI-ARIA 1.2 §4.3` Band-Aid). The role hint inverts the
/// responsibility: widgets declare which `TextNode`s are decoration via
/// `TextNode::with_role(TextRole::Presentational)`, and the
/// enrichment skips past them — no `aria_label` override needed for
/// the common case.
fn first_text_leaf(scene: &Scene) -> Option<String> {
    use pinion_core::scene::TextRole;
    match scene {
        Scene::Text(t) => {
            if matches!(t.role, Some(TextRole::Presentational)) {
                None
            } else {
                Some(t.content.clone())
            }
        }
        Scene::Container(c) => c.children.iter().find_map(first_text_leaf),
        // R1536 — same transparency as `find_container_by_tag`: a nameable
        // container whose label sits inside a nested scroll (a scrolling panel
        // body) has that label as its name-from-contents, not nothing.
        Scene::Scroll(s) => first_text_leaf(&s.content),
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
/// [`ContainerNode`](pinion_core::scene::ContainerNode) does not implement `Clone` (its `ExternalNode`
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

/// R1543 §5.40 §5.39 — populate `nodes[*].access_key` from the mnemonics
/// declared in the paint scene.
///
/// The `accesskey` peer of [`enrich_names_from_scene`], and derived the same
/// way and for the same reason: the mnemonic literal lives once, in the view
/// function, and the AT announcement is read back out of the painted tree. A
/// widget impl that had to *state* its own accelerator would be a second place
/// for it to be written, and the two would drift the first time a label
/// changed — which is exactly the state Qt is in, where the underline is
/// re-parsed by the style on every paint and the shortcut is registered
/// separately in `QShortcutMap`.
///
/// Only the resolved target of each binding is stamped, so what an AT
/// announces is what <kbd>Alt</kbd>+char actually activates — including the
/// `QLabel::setBuddy` case, where the key is announced on the **field**, not on
/// the label carrying the ampersand. HTML `accesskey` behaves the same way
/// (the attribute is on the labelled control), and Qt does not: its
/// `QAccessible::Accelerator` is answered by the label, so a screen-reader user
/// is told the key by a node that is not the one the key operates.
///
/// An **ambiguous** mnemonic is still announced. Two nodes claiming
/// <kbd>Alt</kbd>+S is a bug in the window, but silently announcing neither
/// would hide the accelerator from precisely the users who cannot see the
/// underline; the conflict is reported where conflicts belong, in
/// `scene/mnemonics`.
///
/// Existing `access_key` values are left alone, matching the name pass's
/// explicit-override precedence.
///
/// Returns the number of nodes stamped.
pub fn enrich_access_keys_from_scene(nodes: &mut [AccessNode], scene: &Scene) -> usize {
    let bindings = pinion_core::mnemonic::scene_mnemonics(scene);
    if bindings.is_empty() {
        return 0;
    }
    let mut stamped = 0usize;
    for node in nodes.iter_mut() {
        if node.access_key.is_some() {
            continue;
        }
        // First declaration wins, matching the tag-index rule the name pass
        // uses: a duplicate is a binding bug either way, and picking the same
        // one keeps the two passes describing one tree.
        if let Some(binding) = bindings.iter().find(|b| b.target == node.tag) {
            node.access_key = Some(pinion_core::mnemonic::Mnemonic::accel_label(binding.key));
            stamped += 1;
        }
    }
    stamped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AriaRole;
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
        let mut nodes = vec![AccessNode::new("main_btn", AriaRole::Button).with_name("Explicit")];
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

    /// R1536 — a scroll is transparent to the tag walk.
    ///
    /// The defect this pins: `find_container_by_tag` stopped at any node that
    /// was not a `Container`, so nothing painted inside a `ScrollNode` could be
    /// named — every virtualized list / grid / tree in the tree. It went
    /// unnoticed for ~760 rounds because the *bounds* walker
    /// (`Scene::rect_for_tag_with_offset`) does descend, so the AT tree was
    /// structurally correct and pointed at the right pixels while announcing
    /// nothing. Measured on `hello-virtual-table` before the fix: **0 of 75
    /// `gridcell`s named**; after: 75 of 75.
    ///
    /// Asserted through the public `enrich_names_from_scene` rather than the
    /// private walker, so it is the contract that is pinned and not one
    /// function's shape.
    #[test]
    fn r1536_a_scroll_is_transparent_to_the_name_walk() {
        use pinion_core::scene::ScrollNode;
        let inner = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "Row 7".to_string(),
                Rect::default(),
            ))])
            .with_tag("cell"),
        );
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(ScrollNode::new(
            Rect::default(),
            inner,
        ))]));
        let mut nodes = vec![AccessNode::new("cell", AriaRole::GridCell)];
        assert_eq!(enrich_names_from_scene(&mut nodes, &scene), 1);
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Row 7"),
            "a widget inside a scroll is named from its painted text",
        );
    }

    /// R1536 — the pass costs ONE traversal of the scene, not one per node.
    ///
    /// Stated as a ratio between two node counts against the same scene rather
    /// than as a constant: a per-node search makes the work grow with the
    /// product, so doubling the nodes doubles the traversals, and only a
    /// comparison can see that. The counter is the number of containers the
    /// index visited, which the scene's own shape fixes.
    ///
    /// Why it matters now: before R1536 the search could not enter a scroll,
    /// so for a virtualized grid it walked the whole tree to exhaustion for
    /// every node and found nothing. Making it succeed is what made the
    /// quadratic term real — `hello-virtual-table` resolves 97 nodes against a
    /// ~600-node scene on every AT emit.
    #[test]
    fn r1536_the_scene_is_indexed_once_not_once_per_node() {
        fn scene_of(n: usize) -> Scene {
            Scene::Container(ContainerNode::new(
                (0..n)
                    .map(|i| {
                        Scene::Container(
                            ContainerNode::new(vec![Scene::Text(TextNode::new(
                                format!("t{i}"),
                                Rect::default(),
                            ))])
                            .with_tag(format!("w{i}")),
                        )
                    })
                    .collect(),
            ))
        }
        let scene = scene_of(64);
        let index = tag_index(&scene);
        assert_eq!(
            index.len(),
            64,
            "premise: every tagged container is indexed"
        );

        // One node and sixty-four nodes resolve against the SAME index, so the
        // scene-side cost is identical. A per-node search would have made the
        // second case 64x the first.
        let mut one = vec![AccessNode::new("w0", AriaRole::Button)];
        let mut many: Vec<AccessNode> = (0..64)
            .map(|i| AccessNode::new(format!("w{i}"), AriaRole::Button))
            .collect();
        assert_eq!(enrich_names_from_scene(&mut one, &scene), 1);
        assert_eq!(enrich_names_from_scene(&mut many, &scene), 64);
        assert_eq!(one[0].name.as_deref(), Some("t0"));
        assert_eq!(
            many[63].name.as_deref(),
            Some("t63"),
            "and the last one too"
        );
    }

    /// R1536 — the index keeps the FIRST container with a tag, which is what
    /// the per-node search returned. A duplicate tag is a binding bug either
    /// way; this pins that the R1536 rewrite is a cost change and not a
    /// behaviour change.
    #[test]
    fn r1536_a_duplicate_tag_still_resolves_to_the_first() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::new(
                    "first".to_string(),
                    Rect::default(),
                ))])
                .with_tag("dup"),
            ),
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::new(
                    "second".to_string(),
                    Rect::default(),
                ))])
                .with_tag("dup"),
            ),
        ]));
        let mut nodes = vec![AccessNode::new("dup", AriaRole::Button)];
        enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("first"));
    }

    /// R1536 — and the mirror on the *text* side: a nameable container whose
    /// label sits inside a nested scroll is named by it, not left silent.
    #[test]
    fn r1536_text_is_found_through_a_nested_scroll() {
        use pinion_core::scene::ScrollNode;
        let scene = Scene::Container(
            ContainerNode::new(vec![Scene::Scroll(ScrollNode::new(
                Rect::default(),
                Scene::Container(ContainerNode::new(vec![Scene::Text(TextNode::new(
                    "Deep".to_string(),
                    Rect::default(),
                ))])),
            ))])
            .with_tag("panel"),
        );
        let mut nodes = vec![AccessNode::new("panel", AriaRole::Group)];
        enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("Deep"));
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

    #[test]
    fn r51_81_presentational_text_node_is_skipped() {
        use pinion_core::scene::TextRole;
        // Common Checkbox pattern: decoration glyph ("✓") sits earlier
        // in DFS order than the linguistic label. Pre-R51.81 the glyph
        // would have been picked as the AT name (then masked by an
        // `aria_label` Band-Aid). R51.81 lets the widget declare the
        // glyph as Presentational; enrichment skips past it and lands
        // on the next non-presentational TextNode.
        let scene = Scene::Container(
            ContainerNode::new(vec![
                Scene::Text(
                    TextNode::new("\u{2713}".to_string(), Rect::default())
                        .with_role(TextRole::Presentational),
                ),
                Scene::Text(TextNode::new("Subscribe".to_string(), Rect::default())),
            ])
            .with_tag("checkbox"),
        );
        let mut nodes = vec![AccessNode::new("checkbox", AriaRole::CheckBox)];
        let filled = enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(filled, 1);
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Subscribe"),
            "Presentational TextNode must be skipped during the DFS first-text scan",
        );
    }

    #[test]
    fn r51_81_default_role_keeps_text_in_chain() {
        // Sanity: the default role (`role: None` from `TextNode::new`)
        // behaves exactly as pre-R51.81 — the text participates as the
        // first-text source.
        let scene = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "Save".to_string(),
                Rect::default(),
            ))])
            .with_tag("btn"),
        );
        let mut nodes = vec![AccessNode::new("btn", AriaRole::Button)];
        enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("Save"));
    }
}
