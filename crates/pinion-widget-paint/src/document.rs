//! R1551 §5.36 §5.40 — rich-text **document** composition: a sequence of
//! paragraphs, each with its own [`BlockFormat`].
//!
//! # Why a document is a column of blocks
//!
//! Qt's `QTextDocument` owns a private layout engine (`QTextDocumentLayout`)
//! that stacks blocks and applies their margins itself, which is why a Qt
//! block's indent is invisible to the widget layout around it: the two are
//! different layout systems that meet only at a `QTextEdit`'s viewport.
//!
//! Here a block IS a scene node. Its format lowers to the node's margin
//! ([`TextNode::with_block`]), the ordinary flex pass stacks the column, and the
//! result composes with everything else in the tree — a paragraph can sit beside
//! a widget, inside a splitter, in a scroll — with no document-specific layout
//! code at all, on **both** backends. A `QTextDocument` can do none of that.
//!
//! # One declaration, three consumers
//!
//! [`view_document`] is the only place a [`TextBlock`] becomes a node, so the
//! paint, the `scene/text_blocks` wire and the assistive-technology outline all
//! read the same declaration:
//!
//! * the **paint** takes the block's indents as the node's margin and its
//!   first-line indent through [`TextStyle::text_indent`];
//! * the **wire** (`scene/text_blocks`) reads `TextNode::block` back off the
//!   painted scene, beside where the lines landed;
//! * the **AT outline** (`pinion_a11y::attach_block_headings`) finds a heading
//!   by the same field, named by the same painted text.
//!
//! Tags come from [`DocumentTag`], for the reason every other composite in this
//! crate encodes its tags in one place: a heading is announced by tag, so a
//! second spelling would be a heading that exists on screen and not in the
//! outline.
//!
//! # R1559 — and a list is a column of item rows
//!
//! A block can declare itself a **list item**
//! ([`TextBlock::in_list`]). What it declares is membership — a format and a
//! depth — and never a number, because a number is a property of the item's
//! place among its siblings rather than of the item
//! ([`pinion_core::text_list`]).
//!
//! [`view_document`] therefore does two things a flat block column did not.
//! It **numbers**: one call to
//! [`pinion_core::text_list::number_blocks`] turns the
//! declarations into a marker per item and a run per list. And it **nests**:
//! the runs are rebuilt into real container nodes, so a list is an object with
//! a box (WAI-ARIA `list`, addressable, measurable) and a nested list is inset
//! by its parent's gutter — CSS's own nesting, with no per-level arithmetic
//! anywhere in this file.
//!
//! The marker is an ordinary [`TextNode`]. That is the whole reason the cell
//! backend needs no list code, the shape cache measures markers like any other
//! text, and `scene/text_lists` can publish where a bullet landed. Qt's
//! `QTextDocumentLayout` draws its unordered markers as an ellipse or a
//! rectangle, so in Qt a bullet is not text and none of that follows.

use pinion_core::composite_tag::DocumentTag;
use pinion_core::scene::{ContainerNode, Rect, Scene, StyleRun, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BlockFormat, FlexDirection, LayoutStyle, Size, SizeValue, TextAlign, TextIndent,
    TextStyle,
};
use pinion_core::text_list::{ListNumbering, ListPlacement, ListSpec, number_blocks};

/// One paragraph of a document: its text, its block format, and its inline
/// styling.
///
/// The character style is `Option`: `None` inherits the document's base style,
/// which is the common case and keeps a plain paragraph a one-field
/// construction. That mirrors Qt, where a block's characters carry the
/// document's default `QTextCharFormat` until something overrides them.
#[derive(Debug, Clone)]
pub struct TextBlock {
    /// The paragraph's text. Hard breaks (`U+000A`) inside it stay inside it —
    /// they are line breaks within one block, which is exactly the distinction
    /// CSS `text-indent: each-line` is about.
    pub text: String,
    /// The paragraph's own [`BlockFormat`].
    pub format: BlockFormat,
    /// The paragraph's base character style, or `None` to inherit the
    /// document's.
    pub style: Option<TextStyle>,
    /// Inline styled runs over `text`'s byte ranges (§5.36 rich text).
    pub runs: Vec<StyleRun>,
    /// R1559 — this paragraph is a **list item**: which format, at what depth.
    /// `None` (the default) is an ordinary paragraph.
    ///
    /// Membership only. The author never states a number, because a number is
    /// not something an item has — see
    /// [`pinion_core::text_list::number_blocks`].
    pub list: Option<ListSpec>,
}

impl TextBlock {
    /// A paragraph with the document's base style and no block format — the
    /// plain body paragraph.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: BlockFormat::new(),
            style: None,
            runs: Vec::new(),
            list: None,
        }
    }

    /// Builder: this paragraph's [`BlockFormat`].
    #[must_use]
    pub fn with_format(mut self, format: BlockFormat) -> Self {
        self.format = format;
        self
    }

    /// Builder: override the paragraph's base character style.
    #[must_use]
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Builder: inline styled runs over this paragraph's bytes.
    #[must_use]
    pub fn with_runs(mut self, runs: Vec<StyleRun>) -> Self {
        self.runs = runs;
        self
    }

    /// R1559 builder: this paragraph is an item of a list (Qt
    /// `QTextCursor::createList` / `QTextList::add`).
    #[must_use]
    pub fn in_list(mut self, spec: ListSpec) -> Self {
        self.list = Some(spec);
        self
    }
}

/// Lay `blocks` out as a document: a column of paragraphs, each carrying its own
/// [`BlockFormat`].
///
/// `tag` names the document; each paragraph is tagged
/// [`DocumentTag::block`]`(tag, i)` so a heading can be announced and any
/// paragraph addressed over the `scene/text_blocks` wire.
///
/// The column stretches its paragraphs across whatever width the document is
/// given, so each block's own indents are measured against the space it actually
/// has rather than against a width stated here. A document that does not know
/// how wide it is cannot indent correctly, and the layout engine is what knows.
///
/// # R1559 — list items
///
/// A block that declared membership ([`TextBlock::in_list`]) is emitted as an
/// **item row** ([`DocumentTag::item`]) holding its marker
/// ([`DocumentTag::marker`]) and its paragraph, inside a **list container**
/// ([`DocumentTag::list`]). The numbering is derived here, once, and rides the
/// paragraph node, so the marker on screen, the `aria-posinset` an assistive
/// technology hears and the `scene/text_lists` census are one computation.
///
/// A document with no list membership emits exactly the flat column it did
/// before — no rows, no markers, no containers — so nothing that existed
/// before this changed shape.
#[must_use]
pub fn view_document(tag: &str, base: &TextStyle, blocks: &[TextBlock]) -> ContainerNode {
    let specs: Vec<Option<ListSpec>> = blocks.iter().map(|b| b.list.clone()).collect();
    let numbering = number_blocks(&specs, |k| DocumentTag::list(tag, k));

    let mut root: Vec<Scene> = Vec::new();
    let mut stack: Vec<OpenList> = Vec::new();

    for (i, block) in blocks.iter().enumerate() {
        let style = block.style.clone().unwrap_or_else(|| base.clone());
        let placement = numbering.placements.get(i).and_then(Option::as_ref);
        let mut text = TextNode::styled(block.text.clone(), Rect::new(0, 0, 0, 0), style.clone())
            .with_runs(block.runs.clone())
            .with_block(block.format)
            .with_tag(DocumentTag::block(tag, i));
        let Some(placement) = placement else {
            // An ordinary paragraph ends every open list, which is what
            // `number_blocks` already decided; closing here keeps the painted
            // nesting and the derived numbering one structure.
            close_lists_to(&mut stack, &mut root, 0);
            root.push(Scene::Text(text));
            continue;
        };
        open_lists_for(&mut stack, &mut root, &numbering, &placement.list_tag);
        text = text
            .with_list_placement(placement.clone())
            // The paragraph takes what the marker gutter leaves. Stated on the
            // paragraph rather than as a width on the marker, so a wide marker
            // (`MMMCMXCIX.`) narrows the text instead of overflowing the row.
            .map_layout(|l| l.with_flex_grow(1.0));
        let row = item_row(tag, i, placement, &style, Scene::Text(text));
        push_into(&mut stack, &mut root, row);
    }
    close_lists_to(&mut stack, &mut root, 0);

    ContainerNode::new(root)
        .with_tag(DocumentTag::document(tag))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch),
        )
}

/// The gap between a marker's gutter and its paragraph, in px.
///
/// Part of [`ListFormat::indent_px`](pinion_core::text_list::ListFormat::indent_px)
/// rather than added to it: the declared indent is the whole distance from the
/// list's start edge to its text, which is what CSS's
/// `padding-inline-start` measures and what a reader sees. Splitting it into a
/// marker box plus a gap keeps the marker end-aligned against the text without
/// the two touching.
const MARKER_GAP_PX: u32 = 6;

/// A list container the walk is currently inside.
struct OpenList {
    tag: String,
    /// The gutter this list's items reserve — also how far a list nested in it
    /// is inset, because a nested list starts at its parent's text edge.
    indent_px: u32,
    children: Vec<Scene>,
}

/// Push `node` into the innermost open list, or into the document when there
/// is none.
fn push_into(stack: &mut [OpenList], root: &mut Vec<Scene>, node: Scene) {
    if let Some(open) = stack.last_mut() {
        open.children.push(node);
    } else {
        root.push(node);
    }
}

/// Close open lists until `depth` remain, folding each finished one into its
/// parent (or the document).
fn close_lists_to(stack: &mut Vec<OpenList>, root: &mut Vec<Scene>, depth: usize) {
    while stack.len() > depth {
        let Some(open) = stack.pop() else { return };
        // A nested list is inset by its PARENT's gutter — it begins where its
        // parent's text begins, which is CSS's own nesting and is why no level
        // arithmetic appears anywhere here.
        let inset = stack.last().map_or(0, |parent| parent.indent_px);
        let node = Scene::Container(
            ContainerNode::new(open.children)
                .with_tag(open.tag)
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Stretch)
                        .with_margin(Rect::new(inset, 0, 0, 0)),
                ),
        );
        push_into(stack, root, node);
    }
}

/// Make `list_tag`'s container, and every container enclosing it, the open
/// stack — closing whatever is open below their common ancestry.
fn open_lists_for(
    stack: &mut Vec<OpenList>,
    root: &mut Vec<Scene>,
    numbering: &ListNumbering,
    list_tag: &str,
) {
    let chain = numbering.ancestry(list_tag);
    let shared = stack
        .iter()
        .zip(chain.iter())
        .take_while(|(open, want)| open.tag == **want)
        .count();
    close_lists_to(stack, root, shared);
    for want in chain.iter().skip(shared) {
        let indent_px = numbering
            .run(want)
            .map_or(MARKER_GAP_PX, |run| run.format.indent_px);
        stack.push(OpenList {
            tag: (*want).to_owned(),
            indent_px,
            children: Vec::new(),
        });
    }
}

/// One list item: its marker and its paragraph, side by side.
///
/// The marker is real text in its own addressable node, which is what lets it
/// be measured, copied, shaped, painted by the cell backend, and read back over
/// the wire. Qt draws its unordered markers as an ellipse or a rectangle inside
/// `QTextDocumentLayout`, so none of that is possible there.
fn item_row(
    tag: &str,
    i: usize,
    placement: &ListPlacement,
    style: &TextStyle,
    text: Scene,
) -> Scene {
    let gutter = placement.format.indent_px.saturating_sub(MARKER_GAP_PX);
    let marker = Scene::Text(
        TextNode::styled(
            placement.marker.clone(),
            Rect::new(0, 0, 0, 0),
            // The item's own character style, as Qt's marker takes the block's
            // char format — so a marker matches the size and colour of the
            // text it belongs to. Two paragraph-level fields are dropped: the
            // alignment, because a marker is end-aligned in its gutter by
            // definition, and the text indent, which would push the marker
            // inside its own box.
            style
                .clone()
                .with_align(TextAlign::End)
                .with_text_indent(TextIndent::none()),
        )
        // The number reaches assistive technology as `aria-posinset` on the
        // item (`pinion_a11y::attach_block_lists`), so the glyph itself is
        // decoration — the rule HTML applies to its own `::marker`.
        .with_role(TextRole::Presentational)
        .with_tag(DocumentTag::marker(tag, i))
        .map_layout(|l| {
            l.with_size(Size::auto().with_width(SizeValue::Px(gutter)))
                .with_flex_shrink(0.0)
        }),
    );
    Scene::Container(
        ContainerNode::new(vec![marker, text])
            .with_tag(DocumentTag::item(tag, i))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    // Top-aligned, so a marker sits beside the FIRST line of a
                    // paragraph that wraps rather than being centred against
                    // the whole block.
                    .with_align_items(AlignItems::Start)
                    .with_gap(MARKER_GAP_PX),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::{MARKER_GAP_PX, TextBlock, view_document};
    use pinion_core::composite_tag::DocumentTag;
    use pinion_core::scene::{ContainerNode, Scene, TextNode, TextRole};
    use pinion_core::style::{BlockFormat, SizeValue, TextAlign, TextIndent, TextStyle};
    use pinion_core::text_list::{ListFormat, ListSpec, ListStyle};

    fn doc() -> Vec<TextBlock> {
        vec![
            TextBlock::new("Chapter One").with_format(BlockFormat::new().with_heading_level(1)),
            TextBlock::new("Body.")
                .with_style(TextStyle::new().with_text_indent(TextIndent::first_line(24))),
            TextBlock::new("Quoted.").with_format(BlockFormat::new().with_indent(32)),
        ]
    }

    /// Every paragraph is a tagged text node carrying its own declaration, and
    /// the tags come from the one encode point.
    #[test]
    fn each_block_is_tagged_and_carries_its_declaration() {
        let node = view_document("essay", &TextStyle::new(), &doc());
        assert_eq!(
            node.tag.as_deref(),
            Some(DocumentTag::document("essay").as_str())
        );
        assert_eq!(node.children.len(), 3);
        for (i, child) in node.children.iter().enumerate() {
            let Scene::Text(t) = child else {
                panic!("a block is a text node")
            };
            assert_eq!(
                t.tag.as_deref(),
                Some(DocumentTag::block("essay", i).as_str())
            );
            assert!(t.block.is_some(), "every block carries a format");
        }
    }

    /// The declaration reaches the layout box: the block-quote's indents ARE the
    /// node's margin, which is what lets the ordinary flex pass honour them.
    #[test]
    fn a_block_quote_lowers_its_indents_into_the_margin() {
        let node = view_document("essay", &TextStyle::new(), &doc());
        let Scene::Text(quote) = &node.children[2] else {
            panic!("a block is a text node")
        };
        assert_eq!(quote.layout.margin.x, 32, "left indent");
        assert_eq!(quote.layout.margin.w, 32, "right indent");
        assert_eq!(quote.block.expect("declared").left_indent_px, 32);
    }

    /// R1551 — the block and layout builders are order-independent, so a
    /// paragraph cannot end up declaring an indent it does not have. The
    /// desync this rules out is exactly R1543's on the mnemonic axis: a
    /// declaration bound with its derived ink missing.
    #[test]
    fn the_block_and_layout_builders_are_order_independent() {
        use pinion_core::scene::{Rect, TextNode};
        use pinion_core::style::{AlignItems, FlexDirection, LayoutStyle};
        let fmt = BlockFormat::new().with_indent(32).with_spacing(4, 6);
        let extra = LayoutStyle::new()
            .flex(FlexDirection::Column)
            .with_align_items(AlignItems::Stretch);
        let a = TextNode::new("q", Rect::new(0, 0, 0, 0))
            .with_block(fmt)
            .with_layout(extra);
        let b = TextNode::new("q", Rect::new(0, 0, 0, 0))
            .with_layout(extra)
            .with_block(fmt);
        assert_eq!(a.layout.margin, b.layout.margin, "the same margin");
        assert_eq!(a.layout.margin.x, 32, "and it is the declared indent");
        assert_eq!(
            a.layout.align_items, b.layout.align_items,
            "with the other layout fields kept in both orders",
        );
        let mapped = a.map_layout(|l| l.with_gap(3));
        assert_eq!(mapped.layout.margin.x, 32, "map_layout re-derives it too");
        assert_eq!(mapped.layout.gap, 3, "while keeping what the map set");
    }

    // ── R1559: lists ──────────────────────────────────────────────────────

    fn item(text: &str, level: u8, style: ListStyle) -> TextBlock {
        TextBlock::new(text).in_list(ListSpec::new(ListFormat::new(style)).at_level(level))
    }

    /// Depth-first paint order of every tagged node, so the emitted structure
    /// can be asserted as a whole rather than one probe at a time.
    fn tags(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Container(c) => {
                if let Some(tag) = c.tag.as_deref() {
                    out.push(tag.to_owned());
                }
                for child in &c.children {
                    tags(child, out);
                }
            }
            Scene::Text(t) => {
                if let Some(tag) = t.tag.as_deref() {
                    out.push(tag.to_owned());
                }
            }
            _ => {}
        }
    }

    fn find_text<'a>(scene: &'a Scene, tag: &str) -> Option<&'a TextNode> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t),
            Scene::Container(c) => c.children.iter().find_map(|c| find_text(c, tag)),
            _ => None,
        }
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|c| find_container(c, tag)),
            _ => None,
        }
    }

    /// A list becomes a real subtree: a list container holding one item row
    /// per member, each holding its marker and its paragraph. That structure
    /// is what gives a list a box, a marker a tag, and the nesting somewhere
    /// to live.
    #[test]
    fn r1559_a_list_is_a_container_of_marker_and_paragraph_rows() {
        let blocks = vec![
            TextBlock::new("Intro."),
            item("first", 0, ListStyle::Decimal),
            item("second", 0, ListStyle::Decimal),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let mut found = Vec::new();
        tags(&scene, &mut found);
        assert_eq!(
            found,
            [
                "doc_doc", "doc_blk0", "doc_lst0", "doc_itm1", "doc_mrk1", "doc_blk1", "doc_itm2",
                "doc_mrk2", "doc_blk2",
            ],
        );
        assert_eq!(
            find_text(&scene, "doc_mrk1").expect("a marker").content,
            "1.",
        );
        assert_eq!(
            find_text(&scene, "doc_mrk2").expect("a marker").content,
            "2.",
        );
    }

    /// The defining property, through the composition rather than through the
    /// numbering alone: inserting an item renumbers every PAINTED marker after
    /// it, and none before it.
    #[test]
    fn r1559_inserting_an_item_repaints_the_markers_after_it() {
        let mut blocks = vec![
            item("alpha", 0, ListStyle::Decimal),
            item("beta", 0, ListStyle::Decimal),
            item("gamma", 0, ListStyle::Decimal),
        ];
        let markers = |blocks: &[TextBlock]| -> Vec<String> {
            let scene = Scene::Container(view_document("doc", &TextStyle::new(), blocks));
            (0..blocks.len())
                .filter_map(|i| {
                    find_text(&scene, &DocumentTag::marker("doc", i)).map(|t| t.content.clone())
                })
                .collect()
        };
        assert_eq!(markers(&blocks), ["1.", "2.", "3."]);
        blocks.insert(1, item("inserted", 0, ListStyle::Decimal));
        assert_eq!(
            markers(&blocks),
            ["1.", "2.", "3.", "4."],
            "one insertion, three markers changed — nothing an author wrote",
        );
    }

    /// A nested list is a container inside its parent, inset by the PARENT's
    /// gutter, and the outer list's numbering carries on beneath it.
    #[test]
    fn r1559_a_nested_list_is_inset_by_its_parents_gutter() {
        let outer = ListFormat::numbered().with_indent_px(40);
        let inner = ListFormat::bulleted().with_indent_px(24);
        let blocks = vec![
            TextBlock::new("one").in_list(ListSpec::new(outer.clone())),
            TextBlock::new("deep").in_list(ListSpec::new(inner).at_level(1)),
            TextBlock::new("two").in_list(ListSpec::new(outer)),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let mut found = Vec::new();
        tags(&scene, &mut found);
        assert_eq!(
            found,
            [
                "doc_doc", "doc_lst0", "doc_itm0", "doc_mrk0", "doc_blk0", "doc_lst1", "doc_itm1",
                "doc_mrk1", "doc_blk1", "doc_itm2", "doc_mrk2", "doc_blk2",
            ],
            "the nested list sits INSIDE its parent, between its siblings",
        );
        assert_eq!(
            find_container(&scene, "doc_lst0")
                .expect("the outer list")
                .layout
                .margin
                .x,
            0,
            "a top-level list is not inset",
        );
        assert_eq!(
            find_container(&scene, "doc_lst1")
                .expect("the inner list")
                .layout
                .margin
                .x,
            40,
            "the inner list starts where its PARENT's text starts",
        );
        assert_eq!(
            find_text(&scene, "doc_mrk2").expect("a marker").content,
            "2.",
            "and the outer list carries on under the inner one",
        );
        assert_eq!(
            find_text(&scene, "doc_mrk1").expect("a marker").content,
            "\u{2022}",
        );
    }

    /// The marker's gutter is the declared indent minus the gap, so the whole
    /// distance from the list's edge to its text IS `indent_px` — the number
    /// the author wrote and a reader can measure.
    #[test]
    fn r1559_the_marker_gutter_and_the_gap_are_the_declared_indent() {
        let blocks = vec![
            TextBlock::new("x").in_list(ListSpec::new(ListFormat::numbered().with_indent_px(40))),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let marker = find_text(&scene, "doc_mrk0").expect("a marker");
        assert_eq!(marker.layout.size.width, SizeValue::Px(40 - MARKER_GAP_PX));
        assert_eq!(
            find_container(&scene, "doc_itm0")
                .expect("the item row")
                .layout
                .gap,
            MARKER_GAP_PX,
        );
    }

    /// The marker takes the item's own character style so it matches the text
    /// it belongs to — but not the two paragraph-level fields that would move
    /// it inside its own gutter.
    #[test]
    fn r1559_a_marker_matches_its_item_without_inheriting_its_indent() {
        let styled = TextStyle::new()
            .with_size_px(21)
            .with_text_indent(TextIndent::first_line(30))
            .with_align(TextAlign::Center);
        let blocks = vec![
            TextBlock::new("x")
                .with_style(styled)
                .in_list(ListSpec::new(ListFormat::numbered())),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let marker = find_text(&scene, "doc_mrk0").expect("a marker");
        assert_eq!(marker.style.font_size_px, 21, "the item's own size");
        assert_eq!(marker.style.text_align, TextAlign::End, "end-aligned");
        assert!(marker.style.text_indent.is_none(), "and not indented");
        assert_eq!(marker.role, Some(TextRole::Presentational));
        let text = find_text(&scene, "doc_blk0").expect("the paragraph");
        assert_eq!(text.style.text_indent.amount_px, 30, "the text keeps its");
    }

    /// Every item's paragraph carries the placement, so the a11y pass and the
    /// §7 census read the numbering off the painted scene rather than
    /// recomputing it from a document order neither of them has.
    #[test]
    fn r1559_the_painted_paragraph_carries_its_placement() {
        let blocks = vec![
            item("a", 0, ListStyle::LowerAlpha),
            item("b", 0, ListStyle::LowerAlpha),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let second = find_text(&scene, "doc_blk1").expect("the paragraph");
        let placement = second.list.as_ref().expect("placed");
        assert_eq!(placement.position, 2);
        assert_eq!(placement.count, 2);
        assert_eq!(placement.marker, "b.");
        assert_eq!(placement.list_tag, "doc_lst0");
        assert_eq!(placement.parent_list_tag, None);
        assert_eq!(placement.rendered_as, ListStyle::LowerAlpha);
        let marker = find_text(&scene, "doc_mrk1").expect("a marker");
        assert_eq!(
            marker.content, placement.marker,
            "the painted marker IS the derived one — there is no second source",
        );
    }

    /// The negative control: a document with no list membership paints exactly
    /// what it did before this round — a flat column of paragraphs, no rows,
    /// no markers, no list containers.
    #[test]
    fn r1559_a_document_with_no_items_paints_no_list_machinery() {
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &doc()));
        let mut found = Vec::new();
        tags(&scene, &mut found);
        assert_eq!(found, ["doc_doc", "doc_blk0", "doc_blk1", "doc_blk2"]);
        assert!(
            found
                .iter()
                .all(|t| !t.contains("_lst") && !t.contains("_mrk")),
            "{found:?}",
        );
    }

    /// A paragraph with no style of its own inherits the document's, and one
    /// with a style keeps it — including its paragraph-level text indent.
    #[test]
    fn a_block_inherits_the_document_style_unless_it_states_one() {
        let base = TextStyle::new().with_size_px(21);
        let node = view_document("essay", &base, &doc());
        let Scene::Text(heading) = &node.children[0] else {
            panic!("a block is a text node")
        };
        let Scene::Text(body) = &node.children[1] else {
            panic!("a block is a text node")
        };
        assert_eq!(heading.style.font_size_px, 21, "inherited");
        assert_eq!(body.style.text_indent.amount_px, 24, "its own");
    }
}
