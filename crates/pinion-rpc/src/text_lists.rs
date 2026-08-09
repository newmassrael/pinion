//! `scene/text_lists` — the document's list structure, and the numbering it
//! produced (R1559 §5.12 §5.36 §2 #7).
//!
//! A list is the one text structure whose content is a function of its
//! *sequence*: an item's number is not something the item has, it is where the
//! item sits among its siblings. So the interesting question about a list is
//! never "what does this paragraph say" — `scene/text_blocks` answers that —
//! but "what is in this list, in what order, numbered how". This method
//! answers that: one row per list, each holding its items in order with the
//! marker each one was given and where that marker was painted.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit has the concept and keeps every part of it in-process:
//!
//! - **Enumeration.** There is no "what lists does this document have"
//!   accessor. text document exposes `rootFrame()` and block iteration, and
//!   finding the lists means walking every block calling
//!   `textList()` and de-duplicating the pointers yourself. Here
//!   the census IS the answer, and it is answerable from outside the process.
//! - **Numbering.** `itemNumber()` and `itemText()` are C++ calls
//!   on an object that only exists inside a text document. Neither is
//!   reachable from a driver, a test harness or an agent.
//! - **An unordered marker has no text at all.** the toolkit's text document layout
//!   draws `ListDisc` / `ListCircle` / `ListSquare` as an ellipse or a
//!   rectangle, so `itemText()` has nothing to return for them and no accessor
//!   anywhere reports what the reader is looking at. Every marker here is a
//!   painted string with a tag and a box.
//! - **Geometry.** the toolkit computes a marker's position inside
//!   `drawListItem` and discards it; there is no
//!   per-marker accessor on abstract text document layout. The R1546 rule
//!   applies — the painted extent is published, so "did my marker land in its
//!   gutter" is a question with an answer.
//! - **The fallback is named.** An upper-roman item past 3999 has no roman
//!   form; the toolkit answers `"?"`. CSS Counter Styles Level 3 says render through
//!   the fallback style, so it reads `4000.` here — and
//!   [`TextListItemWire::rendered_as`] states which notation actually wrote
//!   it, so `4000.` in a roman list is distinguishable from `4000.` in a
//!   decimal one.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "lists": [
//!       { "tag": "essay_lst0", "parent_tag": null, "level": 0,
//!         "style": "Decimal", "start": 1, "number_prefix": "",
//!         "number_suffix": ".", "suffix_is_default": true, "indent_px": 40,
//!         "count": 3, "x": 20, "y": 68, "width": 460, "height": 84,
//!         "items": [
//!           { "tag": "essay_blk2", "marker_tag": "essay_mrk2",
//!             "position": 1, "ordinal": 1, "marker": "1.",
//!             "rendered_as": "Decimal", "fell_back": false,
//!             "marker_x": 20, "marker_y": 68,
//!             "marker_width": 34, "marker_height": 19 }
//!         ] }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene, so the
//! lists it reports are the lists on screen.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/text_lists", "id": 1 }
//! ```
//!
//! A binding that paints no list answers with an empty array. That is a
//! legitimate state — most windows have no document in them — and not an
//! error.
//!
//! # Coordinates
//!
//! Window-absolute, with enclosing `Scene::Scroll` offsets folded in exactly
//! as `Scene::rect_for_tag_absolute` folds them, so a list in a scrolled
//! document reports where it is on screen. A list's own box is resolved from
//! its container tag and is `null` when the paint has not laid it out yet —
//! the honest answer for a frame that has been built but not measured, rather
//! than a zero rect a caller would read as "at the origin, empty".

use std::collections::HashMap;

use pinion_core::Scene;
use pinion_core::scene::Rect;
use pinion_core::text_list::ListPlacement;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One painted list: what it declared, and every item in it in order.
#[derive(Debug, Clone, Serialize)]
pub struct TextListWire {
    /// The list container's paint tag (`DocumentTag::list`).
    pub tag: String,
    /// The enclosing list's tag, or `null` at the top level. Walk it to
    /// recover the document's nesting without re-deriving it from `level`.
    pub parent_tag: Option<String>,
    /// Nesting depth; `0` is a top-level list.
    pub level: u8,
    /// The declared marker vocabulary — the toolkit `style()`.
    pub style: String,
    /// The counter value of the first item — the toolkit `setStart`, HTML `<ol start>`.
    pub start: i32,
    /// Text before each counter — the toolkit `numberPrefix()`.
    pub number_prefix: String,
    /// Text after each counter, RESOLVED: the declared suffix, or the style's
    /// default when none was declared.
    pub number_suffix: String,
    /// Whether [`Self::number_suffix`] came from the style rather than from a declaration. The
    /// toolkit spells this distinction as a null string versus an empty one,
    /// which no serialization can carry.
    pub suffix_is_default: bool,
    /// The gutter each item reserves for its marker, and how far a list nested
    /// in this one is inset — px, unlike the toolkit's `indent()` multiplier.
    pub indent_px: u32,
    /// How many items the list holds — the toolkit `count()`.
    pub count: u32,
    /// The list container's window-absolute box, or `null` before layout has
    /// placed it.
    pub x: Option<i64>,
    /// The list container's window-absolute top edge.
    pub y: Option<i64>,
    /// The list container's width.
    pub width: Option<u32>,
    /// The list container's height.
    pub height: Option<u32>,
    /// The items, in document order.
    pub items: Vec<TextListItemWire>,
}

/// One item of a list: its place, its number, and its painted marker.
#[derive(Debug, Clone, Serialize)]
pub struct TextListItemWire {
    /// The item paragraph's paint tag — the same object `scene/text_blocks`
    /// and `scene/access` address, so the three surfaces join on it.
    pub tag: String,
    /// The painted marker's own paint tag, when the marker was painted as a
    /// node. `null` for an item whose marker some other composition drew.
    pub marker_tag: Option<String>,
    /// 1-based position among the list's items — the toolkit `itemNumber() + 1`, and what `aria-posinset`
    /// announces.
    pub position: u32,
    /// The counter value the item was numbered with: the list's `start` plus
    /// its offset. Differs from [`Self::position`] exactly when `start` is not
    /// 1, which is the case a caller must be able to tell apart.
    pub ordinal: i32,
    /// The marker as painted — the toolkit `itemText()`, which has no answer at all for
    /// the unordered styles.
    pub marker: String,
    /// The style whose notation produced [`Self::marker`], after the CSS range
    /// fallback.
    pub rendered_as: String,
    /// Whether the declared style could not represent [`Self::ordinal`] and
    /// the fallback wrote it. Published beside `rendered_as` rather than left
    /// to a string comparison, because a list that DECLARED `decimal` and one
    /// that fell back to it read identically otherwise.
    pub fell_back: bool,
    /// The painted marker's window-absolute left edge, `null` when the marker
    /// has no node or the paint has not placed it.
    pub marker_x: Option<i64>,
    /// The painted marker's window-absolute top edge.
    pub marker_y: Option<i64>,
    /// The painted marker's width.
    pub marker_width: Option<u32>,
    /// The painted marker's height.
    pub marker_height: Option<u32>,
}

/// Response payload for `scene/text_lists`.
#[derive(Debug, Clone, Serialize)]
pub struct TextListsOutcome {
    /// Every painted list, in the order its first item was painted.
    pub lists: Vec<TextListWire>,
}

/// Build the `scene/text_lists` response from the last painted scene.
///
/// # Errors
///
/// A serialization failure, unreachable in practice for owned strings and
/// numbers; surfaced rather than unwrapped so an RPC handler never panics the
/// shell.
pub fn handle_scene_text_lists(last_paint_scene: Option<&Scene>) -> Result<Value, RpcError> {
    let lists = last_paint_scene.map(collect_lists).unwrap_or_default();
    serde_json::to_value(TextListsOutcome { lists }).map_err(RpcError::internal_error)
}

/// Every painted list in `scene`, in the order their first items were painted.
///
/// The whole census reads ONE field — the [`ListPlacement`] the numbering left
/// on each item's text node — so it cannot disagree with the markers on screen
/// or with the `aria-posinset` the a11y pass announces: all three read the
/// same derivation rather than each recomputing it from a document order that
/// only the view has.
#[must_use]
pub fn collect_lists(scene: &Scene) -> Vec<TextListWire> {
    // R1560 — indexed, not scanned. Resolving each marker's box by tag walks
    // the whole scene per item, which is `O(items x scene)`; measured on the
    // sibling `scene/text_tables` census, that lookup was the WHOLE cost of a
    // 5,000-row answer. Same derivation, so the same one traversal.
    let rects = scene.absolute_rects_by_tag();
    let mut lists: Vec<TextListWire> = Vec::new();
    scene.for_each_text_leaf(|node, _, _| {
        let (Some(placement), Some(tag)) = (node.list.as_ref(), node.tag.as_deref()) else {
            return;
        };
        let index = if let Some(index) = lists.iter().position(|l| l.tag == placement.list_tag) {
            index
        } else {
            lists.push(list_row(placement, &rects));
            lists.len() - 1
        };
        // The marker is painted by `DocumentTag::marker` beside this
        // paragraph; deriving its tag from the item's keeps the census from
        // needing a second walk, and an absent rect answers `null` rather than
        // inventing a box.
        let marker_tag = marker_tag_for(tag);
        let marker_rect = marker_tag.as_deref().and_then(|t| rects.get(t).copied());
        if let Some(list) = lists.get_mut(index) {
            list.items.push(TextListItemWire {
                tag: tag.to_owned(),
                marker_tag,
                position: placement.position,
                ordinal: placement.ordinal,
                marker: placement.marker.clone(),
                rendered_as: placement.rendered_as.as_wire().to_owned(),
                fell_back: fell_back(placement),
                marker_x: marker_rect.map(|r| i64::from(r.x)),
                marker_y: marker_rect.map(|r| i64::from(r.y)),
                marker_width: marker_rect.map(|r| r.w),
                marker_height: marker_rect.map(|r| r.h),
            });
        }
    });
    lists
}

/// The declared half of a list's row, plus the box its container was laid out
/// in.
fn list_row(placement: &ListPlacement, rects: &HashMap<String, Rect>) -> TextListWire {
    let format = &placement.format;
    let rect = rects.get(&placement.list_tag).copied();
    TextListWire {
        tag: placement.list_tag.clone(),
        parent_tag: placement.parent_list_tag.clone(),
        level: placement.level,
        style: format.style.as_wire().to_owned(),
        start: format.start,
        number_prefix: format.number_prefix.clone(),
        number_suffix: format.suffix().to_owned(),
        suffix_is_default: format.number_suffix.is_none(),
        indent_px: format.indent_px,
        count: placement.count,
        x: rect.map(|r| i64::from(r.x)),
        y: rect.map(|r| i64::from(r.y)),
        width: rect.map(|r| r.w),
        height: rect.map(|r| r.h),
        items: Vec::new(),
    }
}

/// The marker tag paired with an item paragraph's tag, when the paragraph was
/// tagged by `DocumentTag::block`.
///
/// Derived rather than carried, because the pairing IS
/// [`DocumentTag`](pinion_core::composite_tag::DocumentTag)'s: a block is
/// `"{doc}_blk{i}"` and its marker is `"{doc}_mrk{i}"`. An item whose
/// paragraph was tagged some other way answers `None`, which is why the wire
/// field is nullable — a composition that paints its own markers is a
/// legitimate consumer of the numbering, and this census does not pretend to
/// know where it put them.
fn marker_tag_for(block_tag: &str) -> Option<String> {
    let (doc, index) = block_tag.rsplit_once("_blk")?;
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{doc}_mrk{index}"))
}

/// Whether a marker's declared style could not write its own counter, and the
/// CSS fallback wrote it instead.
///
/// One comparison, stated once, because the wire publishes both the fact and
/// the two styles it is derived from — and a reader who re-derives it from
/// those has to know which of `style` and `rendered_as` is the declaration.
fn fell_back(placement: &ListPlacement) -> bool {
    placement.rendered_as != placement.format.style
}

#[cfg(test)]
mod tests {
    use super::{collect_lists, handle_scene_text_lists, marker_tag_for};
    use pinion_core::Scene;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::style::{LayoutStyle, Size};
    use pinion_core::text_list::{ListFormat, ListSpec, ListStyle, number_blocks};

    /// A painted document: `(text, level)` items plus plain paragraphs, with
    /// the marker nodes `view_document` would paint.
    fn painted(items: &[(&str, Option<u8>)], format: &ListFormat) -> Scene {
        let specs: Vec<Option<ListSpec>> = items
            .iter()
            .map(|(_, level)| level.map(|level| ListSpec::new(format.clone()).at_level(level)))
            .collect();
        let numbering = number_blocks(&specs, |k| format!("doc_lst{k}"));
        let mut children = Vec::new();
        for (i, (text, _)) in items.iter().enumerate() {
            let y = u32::try_from(i).unwrap_or(0) * 20;
            let node = TextNode::new((*text).to_string(), Rect::new(0, y, 200, 20))
                .with_tag(format!("doc_blk{i}"));
            match numbering.placements.get(i).and_then(Option::as_ref) {
                Some(placement) => {
                    children.push(Scene::Text(
                        TextNode::new(placement.marker.clone(), Rect::new(0, 0, 30, 20))
                            .with_tag(format!("doc_mrk{i}"))
                            .map_layout(|l| l.with_size(Size::px(30, 20))),
                    ));
                    children.push(Scene::Text(node.with_list_placement(placement.clone())));
                }
                None => children.push(Scene::Text(node)),
            }
        }
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("doc_lst0_holder")
                .with_layout(LayoutStyle::new()),
        )
    }

    /// A window with no document answers with an empty array, and so does an
    /// unpainted binding — a legitimate state rather than an error.
    #[test]
    fn a_document_with_no_lists_answers_with_an_empty_array() {
        let value = handle_scene_text_lists(None).expect("an unpainted binding answers");
        assert_eq!(value["lists"].as_array().map(Vec::len), Some(0));
        let scene = painted(&[("plain", None)], &ListFormat::numbered());
        let value = handle_scene_text_lists(Some(&scene)).expect("answers");
        assert_eq!(value["lists"].as_array().map(Vec::len), Some(0));
    }

    /// One row per list, holding its items in order with the numbering the
    /// derivation produced.
    #[test]
    fn a_list_reports_its_items_in_order_with_their_numbering() {
        let scene = painted(
            &[("a", Some(0)), ("b", Some(0)), ("c", Some(0))],
            &ListFormat::numbered(),
        );
        let lists = collect_lists(&scene);
        assert_eq!(lists.len(), 1);
        let list = &lists[0];
        assert_eq!(list.tag, "doc_lst0");
        assert_eq!(list.parent_tag, None);
        assert_eq!(list.style, "Decimal");
        assert_eq!(list.number_suffix, ".");
        assert!(list.suffix_is_default, "nothing declared a suffix");
        assert_eq!(list.count, 3);
        assert_eq!(list.items.len(), 3);
        assert_eq!(
            list.items
                .iter()
                .map(|i| i.marker.as_str())
                .collect::<Vec<_>>(),
            ["1.", "2.", "3."],
        );
        assert_eq!(list.items[1].position, 2);
        assert_eq!(list.items[1].ordinal, 2);
        assert_eq!(list.items[1].tag, "doc_blk1");
        assert_eq!(list.items[1].marker_tag.as_deref(), Some("doc_mrk1"));
        assert!(list.items.iter().all(|i| !i.fell_back));
    }

    /// The painted marker's box is published — the thing the toolkit computes
    /// inside a private layout and throws away.
    #[test]
    fn a_markers_painted_box_is_published() {
        let scene = painted(&[("a", Some(0))], &ListFormat::bulleted());
        let lists = collect_lists(&scene);
        let item = &lists[0].items[0];
        assert_eq!(item.marker, "\u{2022}", "a bullet is text here");
        assert_eq!(item.marker_width, Some(30));
        assert_eq!(item.marker_height, Some(20));
        assert_eq!(item.marker_x, Some(0));
    }

    /// `start` moves the counter and not the position; both are published,
    /// because a caller that needs one always needs to tell it from the other.
    #[test]
    fn a_started_list_publishes_position_and_ordinal_apart() {
        let scene = painted(
            &[("a", Some(0)), ("b", Some(0))],
            &ListFormat::numbered().with_start(5),
        );
        let items = &collect_lists(&scene)[0].items;
        assert_eq!((items[1].position, items[1].ordinal), (2, 6));
        assert_eq!(items[1].marker, "6.");
        assert_eq!(collect_lists(&scene)[0].start, 5);
    }

    /// The CSS fallback is REPORTED, so `4000.` in a roman list is distinguishable
    /// from `4000.` in a decimal one — which the toolkit's `"?"` discards and a bare
    /// marker string cannot express.
    #[test]
    fn a_fallen_back_marker_names_the_notation_that_wrote_it() {
        let scene = painted(
            &[("a", Some(0)), ("b", Some(0))],
            &ListFormat::new(ListStyle::UpperRoman).with_start(3999),
        );
        let list = &collect_lists(&scene)[0];
        assert_eq!(list.style, "UpperRoman", "the DECLARATION is unchanged");
        assert_eq!(list.items[0].marker, "MMMCMXCIX.");
        assert_eq!(list.items[0].rendered_as, "UpperRoman");
        assert!(!list.items[0].fell_back);
        assert_eq!(list.items[1].marker, "4000.", "the number is not lost");
        assert_eq!(list.items[1].rendered_as, "Decimal");
        assert!(list.items[1].fell_back, "and the fall is reported");
    }

    /// Nesting is walkable: the inner list names its parent, and the outer
    /// list's own count excludes the inner one's items.
    #[test]
    fn a_nested_list_names_its_parent() {
        let scene = painted(
            &[("one", Some(0)), ("deep", Some(1)), ("two", Some(0))],
            &ListFormat::numbered(),
        );
        let lists = collect_lists(&scene);
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].count, 2);
        assert_eq!(lists[1].level, 1);
        assert_eq!(lists[1].parent_tag.as_deref(), Some(lists[0].tag.as_str()));
        assert_eq!(lists[1].count, 1);
    }

    /// A style has ONE wire spelling. `scene/text_lists` writes it through
    /// `ListStyle::as_wire`, `scene/snapshot` writes the whole placement
    /// through serde's derive, and an agent joining the two surfaces on the
    /// style would be reading two independently-maintained tables. Asserted
    /// per arm rather than in general, because a `#[serde(rename)]` on one
    /// variant is exactly the edit that would split them.
    #[test]
    fn the_two_serializers_spell_a_style_the_same_way() {
        for style in [
            ListStyle::Disc,
            ListStyle::Circle,
            ListStyle::Square,
            ListStyle::Decimal,
            ListStyle::LowerAlpha,
            ListStyle::UpperAlpha,
            ListStyle::LowerRoman,
            ListStyle::UpperRoman,
        ] {
            assert_eq!(
                serde_json::to_value(style).expect("a style serializes"),
                serde_json::Value::String(style.as_wire().to_owned()),
                "{style:?}",
            );
        }
    }

    /// The marker tag is DERIVED from the block tag, so a composition that
    /// tags its paragraphs some other way reports `null` rather than a tag
    /// that resolves to nothing.
    #[test]
    fn a_marker_tag_is_only_derived_from_a_document_block_tag() {
        assert_eq!(
            marker_tag_for("essay_blk12").as_deref(),
            Some("essay_mrk12")
        );
        assert_eq!(marker_tag_for("essay_blk"), None);
        assert_eq!(marker_tag_for("essay_blkx"), None);
        assert_eq!(marker_tag_for("some_button"), None);
    }
}
