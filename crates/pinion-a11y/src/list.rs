//! R1559 §5.40 §5.36 — the document's **list** structure, derived from the
//! paint scene.
//!
//! A paragraph that a numbering derivation placed in a list
//! ([`pinion_core::text_list::ListPlacement`]) becomes a WAI-ARIA `listitem`
//! carrying `aria-posinset`, `aria-setsize` and `aria-level`, inside a `list`
//! node holding the run. That is how a screen-reader user learns "item 3 of 5,
//! level 2" — the information a sighted reader takes from the marker column.
//!
//! ## the toolkit has none of it
//!
//! Not a smaller amount: none. The accessibility interface a text edit
//! implements is accessible text interface, whose vocabulary is character
//! offsets, selections, ranges and text attributes, and which has **no method
//! that reports block structure at all** — the same wall R1551 hit for heading
//! levels. So a toolkit document's lists reach text document layout, which
//! draws their markers, and stop. Worse for the unordered case: the toolkit
//! draws `ListDisc` and friends as painted geometry rather than characters, so a
//! toolkit bullet is not even in the text a screen reader reads out — the item
//! simply begins without any indication that it is one.
//!
//! ## Why a pass over the painted tree
//!
//! The R1543 / R1548 / R1551 shape. **One derivation**: the numbering is
//! computed once, in the view, and rides the painted node, so the outline an
//! assistive technology hears and the markers a reader sees are the same
//! sequence — there is no second "and tell the AT it is item 3" call to
//! forget or to get wrong. **Every topology**: a pass reaches any composition
//! that paints a list item, including ones that do not exist yet.

use pinion_core::Scene;

use crate::{AccessNode, AriaRole, NodeIndex};

/// Emit a WAI-ARIA `list` node per painted list and a `listitem` per item, and
/// return how many nodes the pass added or upgraded.
///
/// # What is announced
///
/// On each **item** (tagged with the paragraph's own tag, so it is the same
/// object `scene/text_blocks` and the heading pass address):
///
/// * the **role** — `listitem`;
/// * `aria-posinset` / `aria-setsize` — its 1-based place and the list's
///   length, which is what an assistive technology reads as "3 of 5";
/// * `aria-level` — 1-based nesting depth, so a nested list is heard as
///   nested rather than as a separate list that happens to follow.
///
/// On each **list**: the `list` role, `aria-setsize` (its length, so the
/// container announces how much is in it before the reader enters), and its
/// items as children in document order.
///
/// # Position, not the printed number
///
/// `aria-posinset` is the item's **position**, so an item printed `7.` in a
/// list that starts at 5 announces as 3. That is what the attribute is defined
/// to carry, and the printed counter is published separately by
/// `scene/text_lists`, where it can be read without being confused for the
/// structural fact.
///
/// # Merging rather than duplicating
///
/// As in [`attach_block_headings`](crate::attach_block_headings): a node that
/// already exists for the tag has its role and set attributes filled in rather
/// than gaining a twin, because two nodes for one tag is a malformed tree. An
/// item that a binding has already described as a heading keeps that role —
/// a heading inside a list is still a heading, and `listitem` is the weaker
/// claim.
pub fn attach_block_lists(nodes: &mut Vec<AccessNode>, scene: &Scene) -> usize {
    /// One list being assembled: its length and its items, in paint order.
    struct Run {
        tag: String,
        count: u32,
        items: Vec<String>,
    }

    let mut runs: Vec<Run> = Vec::new();
    let mut items: Vec<(String, u32, u32, u32)> = Vec::new();
    scene.for_each_text_leaf(|node, _, _| {
        let (Some(placement), Some(tag)) = (node.list.as_ref(), node.tag.as_deref()) else {
            return;
        };
        items.push((
            tag.to_owned(),
            placement.position,
            placement.count,
            u32::from(placement.level).saturating_add(1),
        ));
        if let Some(run) = runs.iter_mut().find(|r| r.tag == placement.list_tag) {
            run.items.push(tag.to_owned());
        } else {
            runs.push(Run {
                tag: placement.list_tag.clone(),
                count: placement.count,
                items: vec![tag.to_owned()],
            });
        }
    });

    let mut touched = 0usize;
    // R1560 — indexed rather than scanned. This pass merged with a linear
    // `find` per item, which R1559 recorded as debt when the table pass
    // inherited the shape and a table's cells made it matter.
    let mut index = NodeIndex::new(nodes);
    for (tag, position, count, level) in items {
        let node = index.upsert(nodes, &tag, AriaRole::ListItem);
        // A heading is the stronger claim and survives; anything else this
        // pass owns, because being an item of a list is what the object IS
        // and the derivation is the authority for that.
        if node.role != AriaRole::Heading {
            node.role = AriaRole::ListItem;
        }
        node.position_in_set = Some(position);
        node.size_of_set = Some(count);
        node.level = Some(level);
        touched += 1;
    }
    for run in runs {
        let node = index.upsert(nodes, &run.tag, AriaRole::List);
        node.role = AriaRole::List;
        node.size_of_set = Some(run.count);
        node.children = run.items;
        touched += 1;
    }
    touched
}

#[cfg(test)]
mod tests {
    use super::attach_block_lists;
    use crate::{AccessNode, AriaRole};
    use pinion_core::Scene;
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
    use pinion_core::style::BlockFormat;
    use pinion_core::text_list::{ListFormat, ListSpec, ListStyle, number_blocks};

    /// Build a painted document from `(text, level or None)` pairs, numbered
    /// the way `view_document` numbers one.
    fn painted(items: &[(&str, Option<u8>)]) -> Scene {
        let specs: Vec<Option<ListSpec>> = items
            .iter()
            .map(|(_, level)| {
                level.map(|level| {
                    let style = if level == 0 {
                        ListStyle::Decimal
                    } else {
                        ListStyle::Disc
                    };
                    ListSpec::new(ListFormat::new(style)).at_level(level)
                })
            })
            .collect();
        let numbering = number_blocks(&specs, |k| format!("doc_lst{k}"));
        let children = items
            .iter()
            .enumerate()
            .map(|(i, (text, _))| {
                let node = TextNode::new((*text).to_string(), Rect::new(0, 0, 100, 20))
                    .with_tag(format!("doc_blk{i}"));
                match numbering.placements.get(i).and_then(Option::as_ref) {
                    Some(placement) => Scene::Text(node.with_list_placement(placement.clone())),
                    None => Scene::Text(node),
                }
            })
            .collect();
        Scene::Container(ContainerNode::new(children))
    }

    fn by_tag<'n>(nodes: &'n [AccessNode], tag: &str) -> &'n AccessNode {
        nodes.iter().find(|n| n.tag == tag).expect("a node")
    }

    /// Each item announces its place and its list's length, and the list
    /// announces its items.
    #[test]
    fn an_item_announces_its_place_in_its_set() {
        let scene = painted(&[("a", Some(0)), ("b", Some(0)), ("c", Some(0))]);
        let mut nodes = Vec::new();
        assert_eq!(
            attach_block_lists(&mut nodes, &scene),
            4,
            "three items and the list they are in",
        );
        let second = by_tag(&nodes, "doc_blk1");
        assert_eq!(second.role, AriaRole::ListItem);
        assert_eq!(second.position_in_set, Some(2));
        assert_eq!(second.size_of_set, Some(3));
        assert_eq!(second.level, Some(1), "a top-level item is aria-level 1");
        let list = by_tag(&nodes, "doc_lst0");
        assert_eq!(list.role, AriaRole::List);
        assert_eq!(list.size_of_set, Some(3));
        assert_eq!(list.children, ["doc_blk0", "doc_blk1", "doc_blk2"]);
    }

    /// The counterfactual: an ordinary paragraph is not an item, and an
    /// untagged one cannot be addressed so it is not announced either.
    #[test]
    fn a_paragraph_outside_a_list_announces_nothing() {
        let scene = painted(&[("plain", None), ("also plain", None)]);
        let mut nodes = Vec::new();
        assert_eq!(attach_block_lists(&mut nodes, &scene), 0);
        assert!(nodes.is_empty());
    }

    /// A nested item is heard as nested — the level, its own list's length,
    /// and its own list node.
    #[test]
    fn a_nested_item_announces_its_depth_and_its_own_set() {
        let scene = painted(&[
            ("outer one", Some(0)),
            ("inner a", Some(1)),
            ("inner b", Some(1)),
            ("outer two", Some(0)),
        ]);
        let mut nodes = Vec::new();
        attach_block_lists(&mut nodes, &scene);
        let inner = by_tag(&nodes, "doc_blk1");
        assert_eq!(inner.level, Some(2), "one level in is aria-level 2");
        assert_eq!(inner.position_in_set, Some(1));
        assert_eq!(inner.size_of_set, Some(2), "of the INNER list");
        let outer_two = by_tag(&nodes, "doc_blk3");
        assert_eq!(
            outer_two.position_in_set,
            Some(2),
            "the outer list carries on under the inner one",
        );
        assert_eq!(outer_two.size_of_set, Some(2));
        assert_eq!(by_tag(&nodes, "doc_lst1").size_of_set, Some(2));
        assert_eq!(
            by_tag(&nodes, "doc_lst1").children,
            ["doc_blk1", "doc_blk2"]
        );
    }

    /// R1536's arm: a document long enough to have lists is a document inside a
    /// scroll, and the leaf walk descends into one.
    #[test]
    fn an_item_inside_a_scroll_is_found() {
        let scene = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 50),
            painted(&[("deep", Some(0))]),
        ));
        let mut nodes = Vec::new();
        assert_eq!(attach_block_lists(&mut nodes, &scene), 2);
        assert_eq!(by_tag(&nodes, "doc_blk0").role, AriaRole::ListItem);
    }

    /// A binding that already describes the tag gets its set attributes filled
    /// in rather than a second node for the same object — and a heading inside
    /// a list stays a heading, because that is the stronger claim.
    #[test]
    fn an_existing_node_is_upgraded_and_a_heading_keeps_its_role() {
        let scene = painted(&[("one", Some(0)), ("Title", Some(0))]);
        let mut nodes = vec![
            AccessNode::new("doc_blk0", AriaRole::Generic).with_name("Explicit"),
            AccessNode::new("doc_blk1", AriaRole::Heading).with_level(2),
        ];
        attach_block_lists(&mut nodes, &scene);
        assert_eq!(nodes.len(), 3, "two items and one list, no twins");
        let first = by_tag(&nodes, "doc_blk0");
        assert_eq!(first.role, AriaRole::ListItem);
        assert_eq!(first.name.as_deref(), Some("Explicit"), "the name survives");
        let heading = by_tag(&nodes, "doc_blk1");
        assert_eq!(heading.role, AriaRole::Heading);
        assert_eq!(heading.position_in_set, Some(2), "and is still item 2");
    }

    /// Two adjacent lists are two nodes with two lengths — the property that
    /// separates a real list structure from "every item in the document".
    #[test]
    fn two_lists_are_two_sets() {
        let scene = painted(&[
            ("a", Some(0)),
            ("b", Some(0)),
            ("break", None),
            ("c", Some(0)),
        ]);
        let mut nodes = Vec::new();
        attach_block_lists(&mut nodes, &scene);
        assert_eq!(by_tag(&nodes, "doc_blk1").size_of_set, Some(2));
        assert_eq!(by_tag(&nodes, "doc_blk3").size_of_set, Some(1));
        assert_eq!(
            by_tag(&nodes, "doc_blk3").position_in_set,
            Some(1),
            "the second list starts again",
        );
        assert_eq!(by_tag(&nodes, "doc_lst1").children, ["doc_blk3"]);
    }

    /// An item with no `BlockFormat` is still an item — the two declarations
    /// are independent, and a list item is not required to be a special block.
    #[test]
    fn list_membership_is_independent_of_the_block_format() {
        let mut scene = painted(&[("q", Some(0))]);
        if let Scene::Container(c) = &mut scene
            && let Some(Scene::Text(t)) = c.children.first_mut()
        {
            *t = t.clone().with_block(BlockFormat::new().with_indent(8));
        }
        let mut nodes = Vec::new();
        assert_eq!(attach_block_lists(&mut nodes, &scene), 2);
    }
}
