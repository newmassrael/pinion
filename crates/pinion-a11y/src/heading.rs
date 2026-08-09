//! R1551 §5.40 §5.36 — the document outline, derived from the paint scene.
//!
//! A paragraph that declares itself a heading
//! ([`pinion_core::style::BlockFormat::heading_level`]) becomes a WAI-ARIA
//! `heading` node carrying
//! `aria-level`, so a screen-reader user can navigate a document by its
//! structure — the primary way long documents are read non-visually.
//!
//! ## Why a pass over the painted tree
//!
//! The R1543 / R1548 shape, for the same two reasons. **One declaration**: the
//! heading level is stated once, on the block format that also indents and
//! spaces the paragraph, so the announced outline and the drawn one cannot
//! disagree — there is no second "and mark this one a heading" call for a
//! binding to forget. **Every topology**: a pass reaches any composition that
//! paints a block, including ones that do not exist yet, where a parameter
//! threaded through a builder reaches only that builder.
//!
//! ## the toolkit has the declaration and not the announcement
//!
//! `setHeadingLevel()` has existed since the toolkit 5.15 and the toolkit's Markdown reader
//! sets it. But the accessibility surface a text edit implements is accessible
//! text interface, whose vocabulary is character offsets, selections, ranges
//! and text attributes; it has **no method that reports block structure**, so
//! those heading levels reach text document layout (which draws them larger)
//! and stop. The information exists in the document and never reaches the user
//! who most needs it.
//!
//! ## The walk is [`Scene::for_each_text_leaf`]
//!
//! R1551 wrote this pass its own recursive walk and R1559 replaced it with the
//! shared one, which had gained a third caller by then. They were the same
//! traversal — container children, transparent `Scroll`, text leaves — and the
//! `Scroll` arm is the load-bearing one: R1536 measured what its absence cost
//! (nothing painted inside a scroll could be named at all, while the bounds
//! walker descended fine and left the tree looking correct), and a document
//! long enough to have headings is exactly a document inside a scroll. One
//! copy of that arm is one place for it to be right.

use pinion_core::Scene;
use pinion_core::scene::TextNode;

use crate::{AccessNode, AriaRole, NodeIndex};

/// Emit a WAI-ARIA `heading` node for every painted paragraph that declares a
/// heading level, and return how many nodes the pass added or upgraded.
///
/// # What is announced
///
/// * the **role** — `heading`;
/// * the **level** — `aria-level`, clamped to `1..=6` by
///   [`aria_level`](pinion_core::style::BlockFormat::aria_level), so a declared
///   `9` announces as a level
///   assistive technology has a name for while the declaration keeps what the
///   author wrote;
/// * the **name** — the paragraph's *painted* first line. R1547's rule: the
///   announced string is the drawn one, because there is no second source it
///   could be taken from.
///
/// # A heading is addressed by its tag
///
/// Only a [`TextNode`] carrying a `tag` becomes a node. That is not a special
/// case for headings — every object in this tree is addressed by tag (the
/// focus manager, the bounds resolver and the `scene/access` dump all key on
/// it), and a node with no tag could neither be placed on screen nor be
/// referred to twice running. The document composition
/// (`pinion_widget_paint::view_document`) tags each block from one encode
/// function, so a heading built that way always has one.
///
/// # Merging rather than duplicating
///
/// If `nodes` already holds a node with the paragraph's tag — a binding that
/// describes that text itself — the pass sets its role and level instead of
/// appending a second node for the same object. Two nodes for one tag is a
/// malformed tree; the declaration on the block format is the authority for
/// what that object *is*.
pub fn attach_block_headings(nodes: &mut Vec<AccessNode>, scene: &Scene) -> usize {
    let mut found: Vec<(&str, u8, &str)> = Vec::new();
    scene.for_each_text_leaf(|t, _, _| {
        if let Some((tag, level)) = heading_of(t) {
            found.push((tag, level, t.content.as_str()));
        }
    });
    let mut touched = 0usize;
    let mut index = NodeIndex::new(nodes);
    for (tag, level, content) in found {
        let level = u32::from(level);
        let node = index.upsert(nodes, tag, AriaRole::Heading);
        node.role = AriaRole::Heading;
        node.level = Some(level);
        if node.name.is_none() {
            node.name = Some(first_line(content).to_string());
        }
        touched += 1;
    }
    touched
}

/// The `(tag, aria level)` of a text node that is an addressable heading.
fn heading_of(t: &TextNode) -> Option<(&str, u8)> {
    let level = t.block?.aria_level()?;
    Some((t.tag.as_deref()?, level))
}

/// A heading's announced name is its first painted line, the rule
/// `enrich_names_from_scene` applies to every other node's name.
fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::attach_block_headings;
    use crate::{AccessNode, AriaRole};
    use pinion_core::Scene;
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
    use pinion_core::style::BlockFormat;

    fn heading(tag: &'static str, text: &str, level: u8) -> Scene {
        Scene::Text(
            TextNode::new(text.to_string(), Rect::new(0, 0, 100, 20))
                .with_block(BlockFormat::new().with_heading_level(level))
                .with_tag(tag),
        )
    }

    #[test]
    fn a_declared_heading_becomes_a_heading_node_named_by_its_paint() {
        let scene = Scene::Container(ContainerNode::new(vec![heading("h", "Chapter One", 1)]));
        let mut nodes = Vec::new();
        assert_eq!(attach_block_headings(&mut nodes, &scene), 1);
        assert_eq!(nodes[0].role, AriaRole::Heading);
        assert_eq!(nodes[0].level, Some(1));
        assert_eq!(nodes[0].name.as_deref(), Some("Chapter One"));
    }

    /// The counterfactual: an ordinary paragraph, and a heading paragraph with
    /// no tag, are both silent. The tag rule is what makes a heading
    /// addressable, so it is asserted rather than assumed.
    #[test]
    fn an_untagged_or_plain_paragraph_announces_nothing() {
        let plain = Scene::Text(
            TextNode::new("Body text".to_string(), Rect::new(0, 0, 100, 20)).with_tag("p"),
        );
        let untagged = Scene::Text(
            TextNode::new("Loud".to_string(), Rect::new(0, 0, 100, 20))
                .with_block(BlockFormat::new().with_heading_level(2)),
        );
        let scene = Scene::Container(ContainerNode::new(vec![plain, untagged]));
        let mut nodes = Vec::new();
        assert_eq!(attach_block_headings(&mut nodes, &scene), 0);
        assert!(nodes.is_empty());
    }

    /// R1536's arm, on this walk: a document long enough to have headings is a
    /// document inside a scroll.
    #[test]
    fn a_heading_inside_a_scroll_is_found() {
        let scene = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 50),
            Scene::Container(ContainerNode::new(vec![heading("h", "Deep", 3)])),
        ));
        let mut nodes = Vec::new();
        assert_eq!(attach_block_headings(&mut nodes, &scene), 1);
        assert_eq!(nodes[0].level, Some(3));
    }

    /// A level past the ARIA vocabulary announces as 6; the declaration keeps
    /// what the author wrote (asserted on the format, not on the node).
    #[test]
    fn an_out_of_range_level_announces_within_the_vocabulary() {
        let scene = Scene::Container(ContainerNode::new(vec![heading("h", "Deep", 9)]));
        let mut nodes = Vec::new();
        attach_block_headings(&mut nodes, &scene);
        assert_eq!(nodes[0].level, Some(6));
        assert_eq!(
            BlockFormat::new().with_heading_level(9).heading_level,
            9,
            "the declaration is not clamped, only its announcement",
        );
    }

    /// A binding that already describes the tag gets its role upgraded rather
    /// than a second node for the same object.
    #[test]
    fn an_existing_node_is_upgraded_not_duplicated() {
        let scene = Scene::Container(ContainerNode::new(vec![heading("h", "Title", 2)]));
        let mut nodes = vec![AccessNode::new("h", AriaRole::Generic).with_name("Explicit")];
        assert_eq!(attach_block_headings(&mut nodes, &scene), 1);
        assert_eq!(nodes.len(), 1, "one object, one node");
        assert_eq!(nodes[0].role, AriaRole::Heading);
        assert_eq!(nodes[0].level, Some(2));
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Explicit"),
            "an explicit name still wins, as it does for every other pass",
        );
    }
}
