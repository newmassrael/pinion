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

use pinion_core::composite_tag::DocumentTag;
use pinion_core::scene::{ContainerNode, Rect, Scene, StyleRun, TextNode};
use pinion_core::style::{AlignItems, BlockFormat, FlexDirection, LayoutStyle, TextStyle};

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
#[must_use]
pub fn view_document(tag: &str, base: &TextStyle, blocks: &[TextBlock]) -> ContainerNode {
    let children: Vec<Scene> = blocks
        .iter()
        .enumerate()
        .map(|(i, block)| {
            let style = block.style.clone().unwrap_or_else(|| base.clone());
            Scene::Text(
                TextNode::styled(block.text.clone(), Rect::new(0, 0, 0, 0), style)
                    .with_runs(block.runs.clone())
                    .with_block(block.format)
                    .with_tag(DocumentTag::block(tag, i)),
            )
        })
        .collect();
    ContainerNode::new(children)
        .with_tag(DocumentTag::document(tag))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch),
        )
}

#[cfg(test)]
mod tests {
    use super::{TextBlock, view_document};
    use pinion_core::composite_tag::DocumentTag;
    use pinion_core::scene::Scene;
    use pinion_core::style::{BlockFormat, TextIndent, TextStyle};

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
