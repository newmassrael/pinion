//! `scene/text_blocks` — what each paragraph DECLARED about itself, and where
//! its lines actually landed (R1551 §5.12 §5.36 §2 #7).
//!
//! A [`BlockFormat`] is a declaration — "indent me 24px, put 8px above me, I am
//! a level-2 heading". It lowers into a layout margin and, for the first-line
//! indent, into how the shaper breaks the paragraph. Neither lowering can be
//! read back: a margin is a margin whether a paragraph asked for it or its
//! container did, and a line's x tells you where it starts, not why. So this
//! method publishes the two halves side by side — the declaration and the
//! resolved line boxes — which is the only form in which "did my indent reach
//! the layout" is a question with an answer.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit has both halves and joins neither to the other, nor exposes
//! either as data.
//!
//! - **The declaration** lives in text block format, reachable only in-process
//!   through a text cursor, and only as a **property bag**:
//!   `property(int)` returns a dynamic value, an unset property
//!   returns an invalid one, and the typed getters silently substitute a
//!   default. So a toolkit program cannot enumerate what a block declared — only ask
//!   about properties it already thought to name. Here the block's whole
//!   content is a struct, and the wire carries every field of it.
//! - **The units** are not one unit in the toolkit. `indent()` is an `int` multiplied by
//!   the document-wide `indentWidth`, while `leftMargin()` and
//!   its siblings are `qreal` pixels — two scales in one class, with nothing on
//!   the value saying which it is. Everything below is px.
//! - **The geometry** lives in a different object again
//!   (`blockBoundingRect`), is not published, and
//!   stops at the block: the toolkit has no accessor for where an individual *line*
//!   landed after alignment and indentation. `x()` exists but only
//!   while you hold the text layout, which text document owns privately.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "blocks": [
//!       { "tag": "doc#1",
//!         "x": 16, "y": 48, "width": 360, "height": 40,
//!         "block": { "left_indent_px": 24, "right_indent_px": 24,
//!                    "space_above_px": 8, "space_below_px": 8,
//!                    "heading_level": 0, "aria_level": null },
//!         "text_indent": { "amount_px": 24, "hanging": false,
//!                          "each_line": false },
//!         "align": "Start",
//!         "lines": [ { "start": 0, "end": 21, "x": 24, "y": 0,
//!                      "advance": 148.5, "trailing_whitespace": 4.5,
//!                      "height": 19 } ] }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene, so the
//! blocks it reports are the blocks on screen.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/text_blocks", "id": 1 }
//! ```
//!
//! # What counts as a block
//!
//! A painted text leaf appears here when it declares a [`BlockFormat`] **or** a
//! non-zero [`TextIndent`] — the two halves of the paragraph-level declaration.
//! Reporting one without the other would leave an indent unverifiable, which is
//! the one thing this method exists for. `block` is `null` for a paragraph that
//! declares only an indent; that is the honest answer, not a zero-filled
//! format it never asked for.
//!
//! An ordinary label declares neither and is not a document block, so it is
//! absent. A binding with no paragraphs answers with an empty list, which is a
//! legitimate state and not an error.
//!
//! # Coordinates
//!
//! The block's `x` / `y` are window-absolute, with enclosing `Scene::Scroll`
//! offsets folded in exactly as `Scene::rect_for_tag_absolute` folds them. A
//! **line's** `x` / `y` are relative to the block, because that is the frame
//! the question is asked in: an indent is a distance from the paragraph's own
//! start edge, and making a consumer subtract the block origin to recover it
//! would put the arithmetic on the wrong side of the wire.
//!
//! [`BlockFormat`]: pinion_core::style::BlockFormat
//! [`TextIndent`]: pinion_core::style::TextIndent

use pinion_core::scene::{Scene, TextNode};
use pinion_core::style::{BlockFormat, TextIndent};
use pinion_text::LayoutCache;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One painted paragraph: what it declared, and where its lines landed.
#[derive(Debug, Clone, Serialize)]
pub struct TextBlockReport {
    /// The paint tag of the paragraph's text node, when it has one. A heading
    /// needs one to reach assistive technology (see
    /// `pinion_a11y::attach_block_headings`); an ordinary block does not.
    pub tag: Option<String>,
    /// Window-absolute x of the paragraph's box — after its left indent, since
    /// the indent is a margin and a margin is outside the box.
    pub x: i64,
    /// Window-absolute y of the paragraph's box.
    pub y: i64,
    /// The paragraph box's width.
    pub width: u32,
    /// The paragraph box's height.
    pub height: u32,
    /// The declared [`BlockFormat`], or `null` when the paragraph declares only
    /// a text indent.
    ///
    /// [`BlockFormat`]: pinion_core::style::BlockFormat
    pub block: Option<BlockFormatWire>,
    /// The declared CSS `text-indent`.
    pub text_indent: TextIndentWire,
    /// The paragraph's CSS `text-align`, in the spelling `scene/snapshot` uses.
    pub align: String,
    /// Where each shaped line landed, top to bottom — parley's own line
    /// metrics, so these are the painter's numbers and not a second opinion.
    pub lines: Vec<TextLineWire>,
}

/// A declared [`BlockFormat`] on the wire.
///
/// [`BlockFormat`]: pinion_core::style::BlockFormat
///
/// Every length is CSS px. `aria_level` is the announcement derived from
/// `heading_level` — published beside it rather than instead of it, because the
/// two differ exactly where a caller most needs to see both: a declared level
/// past the ARIA vocabulary announces clamped while the declaration keeps what
/// the author wrote.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BlockFormatWire {
    /// The toolkit `setLeftMargin` + `setIndent`, in px.
    pub left_indent_px: u32,
    /// The toolkit `setRightMargin`, in px.
    pub right_indent_px: u32,
    /// The toolkit `setTopMargin`, in px.
    pub space_above_px: u32,
    /// The toolkit `setBottomMargin`, in px.
    pub space_below_px: u32,
    /// The toolkit `setHeadingLevel`; `0` = not a heading.
    pub heading_level: u8,
    /// The WAI-ARIA `aria-level` this block announces, `null` when it is not a
    /// heading.
    pub aria_level: Option<u8>,
}

impl From<BlockFormat> for BlockFormatWire {
    fn from(b: BlockFormat) -> Self {
        Self {
            left_indent_px: b.left_indent_px,
            right_indent_px: b.right_indent_px,
            space_above_px: b.space_above_px,
            space_below_px: b.space_below_px,
            heading_level: b.heading_level,
            aria_level: b.aria_level(),
        }
    }
}

/// A declared CSS `text-indent` on the wire.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TextIndentWire {
    /// Signed px; negative outdents.
    pub amount_px: i32,
    /// CSS `hanging` — indent the continuation lines instead of the first.
    pub hanging: bool,
    /// CSS `each-line` — re-apply after every hard break in the block.
    pub each_line: bool,
}

impl From<TextIndent> for TextIndentWire {
    fn from(i: TextIndent) -> Self {
        Self {
            amount_px: i.amount_px,
            hanging: i.hanging,
            each_line: i.each_line,
        }
    }
}

/// One shaped line's box, in the paragraph's own frame.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TextLineWire {
    /// UTF-8 byte offset of the line's first byte.
    pub start: u32,
    /// UTF-8 byte offset one past its last byte.
    pub end: u32,
    /// Where the line starts, relative to the paragraph's box — parley's
    /// `LineMetrics::offset`, which is where BOTH the text indent and the
    /// alignment land. A first-line indent shows up here and nowhere else.
    pub x: f64,
    /// The line's top edge, relative to the paragraph's box.
    pub y: f64,
    /// Full advance, trailing whitespace included.
    pub advance: f64,
    /// Advance of the trailing whitespace, so a caller can compute the inked
    /// width (`advance - trailing_whitespace`) the way alignment does.
    pub trailing_whitespace: f64,
    /// The line box's height.
    pub height: f64,
}

/// Response payload for `scene/text_blocks`.
#[derive(Debug, Clone, Serialize)]
pub struct TextBlocksOutcome {
    /// Every painted paragraph, in paint order.
    pub blocks: Vec<TextBlockReport>,
}

/// Typed errors [`handle_scene_text_blocks`] can return. The variant name rides
/// in `error.data` so an agent pattern-matches rather than parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextBlocksError {
    /// The embedder installed no block list on the dispatch context.
    ///
    /// Distinct from an empty list, and the distinction is the point: empty
    /// means "this frame paints no paragraphs", while this means "this host
    /// cannot answer" — a host that never shapes (`pinion-tui`) or a fixture
    /// with no shape cache. A caller asserting "every indent landed" must not
    /// read the second as the first.
    TextBlocksUnavailable,
}

/// Collect every painted paragraph in `scene`, in paint order.
///
/// Called by the embedder before dispatch (the `text_backgrounds` pattern),
/// because the answer needs BOTH the painted scene and the shape cache and only
/// the shell holds them together.
///
/// `cache` must be the shell's own — the one the painter shaped through — so
/// every line box here comes off an entry the frame already derived. That is
/// what makes these the painter's numbers rather than a second shaping that
/// could disagree with the pixels.
#[must_use]
pub fn collect_blocks(scene: &Scene, cache: &mut LayoutCache) -> Vec<TextBlockReport> {
    // Two passes rather than one closure that both walks and shapes: the walk
    // borrows `scene` while the shaping borrows `cache` mutably, and collecting
    // the leaves first keeps those borrows apart without threading the cache
    // through the visitor.
    let mut leaves: Vec<(&TextNode, i64, i64)> = Vec::new();
    scene.for_each_text_leaf(|t, x, y| {
        if is_block(t) {
            leaves.push((t, x, y));
        }
    });
    leaves
        .into_iter()
        .map(|(t, x, y)| report(t, x, y, cache))
        .collect()
}

/// Build the `scene/text_blocks` response from the list the embedder collected
/// with [`collect_blocks`].
///
/// # Errors
///
/// - [`TextBlocksError::TextBlocksUnavailable`] — the embedder installed no
///   block list.
/// - A serialization failure, unreachable in practice for owned strings and
///   numbers; surfaced rather than unwrapped so an RPC handler never panics the
///   shell.
pub fn handle_scene_text_blocks(blocks: Option<&[TextBlockReport]>) -> Result<Value, RpcError> {
    let Some(blocks) = blocks else {
        return Err(
            RpcError::invalid_params("text blocks unavailable for this embedder")
                .with_data_string("TextBlocksUnavailable"),
        );
    };
    serde_json::to_value(TextBlocksOutcome {
        blocks: blocks.to_vec(),
    })
    .map_err(RpcError::internal_error)
}

/// Whether `t` declares itself a paragraph: a block format, a text indent, or
/// both.
fn is_block(t: &TextNode) -> bool {
    // ★★★★★ R1780 — a declared ALIGNMENT makes a paragraph too, and until this
    // round it did not.
    //
    // The two halves of the wire were saying different kinds of thing about the
    // same node. `scene/snapshot` reports the `text_align` that was ASKED FOR,
    // for any text node. This surface reports where each line actually LANDED —
    // and its own field documentation says the line's `x` is "where BOTH the
    // text indent and the alignment land". So the one place a client could read
    // an alignment's EFFECT excluded exactly the nodes whose only declaration
    // was an alignment: the wire said what was requested and never what
    // happened.
    //
    // That is the shape a debt recorded for 84 rounds as "a declared alignment
    // does nothing" — measured at R1780, it does exactly what it says, within
    // the width the node was given, and what was missing was any way to SEE
    // that from outside.
    //
    // `Start` is excluded because it is the default: reporting every unstyled
    // label would drown the paragraphs this surface exists for, and a default
    // nobody asked for is not a declaration.
    t.block.is_some()
        || !t.style.text_indent.is_none()
        || t.style.text_align != pinion_core::style::TextAlign::Start
}

/// Report one painted paragraph.
///
/// `(x_off, y_off)` is the window-absolute offset `Scene::for_each_text_leaf`
/// resolved for this leaf, so a paragraph inside a scroll reports where it is
/// on screen rather than where it is in its own content tree.
fn report(t: &TextNode, x_off: i64, y_off: i64, cache: &mut LayoutCache) -> TextBlockReport {
    let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
    let layout = cache.layout_with_runs(&t.content, &t.style, &t.runs, max_width);
    let lines = pinion_text::line_boxes(layout)
        .into_iter()
        .map(|l| TextLineWire {
            start: l.start,
            end: l.end,
            x: f64::from(l.x),
            y: f64::from(l.y),
            advance: f64::from(l.advance),
            trailing_whitespace: f64::from(l.trailing_whitespace),
            height: f64::from(l.height),
        })
        .collect();
    TextBlockReport {
        tag: t.tag.as_ref().map(std::string::ToString::to_string),
        x: x_off + i64::from(t.rect.x),
        y: y_off + i64::from(t.rect.y),
        width: t.rect.w,
        height: t.rect.h,
        block: t.block.map(Into::into),
        text_indent: t.style.text_indent.into(),
        align: t.style.text_align.as_wire().to_owned(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_blocks, handle_scene_text_blocks};
    use pinion_core::Scene;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::style::{BlockFormat, TextIndent, TextStyle};
    use pinion_text::LayoutCache;

    /// An unavailable slot is not an empty list — the distinction the error
    /// variant exists for.
    #[test]
    fn an_absent_slot_is_an_error_not_an_empty_list() {
        let err = handle_scene_text_blocks(None).expect_err("no slot installed");
        assert_eq!(
            err.data,
            Some(serde_json::Value::String(
                "TextBlocksUnavailable".to_owned()
            )),
        );
        let ok = handle_scene_text_blocks(Some(&[])).expect("an empty frame answers");
        assert_eq!(ok["blocks"].as_array().map(Vec::len), Some(0));
    }

    /// A plain label is not a document block; a paragraph that declares only an
    /// indent is, and reports `block: null` rather than a format it never asked
    /// for.
    #[test]
    fn only_declared_paragraphs_are_reported() {
        let label = Scene::Text(TextNode::new("plain".to_string(), Rect::new(0, 0, 200, 20)));
        let indented = Scene::Text(TextNode::styled(
            "indented".to_string(),
            Rect::new(0, 20, 200, 20),
            TextStyle::new().with_text_indent(TextIndent::first_line(24)),
        ));
        let formatted = Scene::Text(
            TextNode::new("quote".to_string(), Rect::new(0, 40, 200, 20))
                .with_block(BlockFormat::new().with_indent(24)),
        );
        let scene = Scene::Container(ContainerNode::new(vec![label, indented, formatted]));
        let mut cache = LayoutCache::new();
        let blocks = collect_blocks(&scene, &mut cache);
        assert_eq!(blocks.len(), 2, "the plain label is not a paragraph");
        assert!(blocks[0].block.is_none());
        assert_eq!(blocks[0].text_indent.amount_px, 24);
        assert_eq!(blocks[1].block.expect("declared").left_indent_px, 24);
        assert_eq!(blocks[1].text_indent.amount_px, 0);
    }

    /// A declared heading publishes BOTH the level it declared and the level it
    /// announces, which differ exactly where the caller needs to see both.
    #[test]
    fn a_heading_publishes_its_declared_and_announced_levels() {
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Text(
            TextNode::new("Title".to_string(), Rect::new(0, 0, 200, 20))
                .with_block(BlockFormat::new().with_heading_level(9)),
        )]));
        let mut cache = LayoutCache::new();
        let blocks = collect_blocks(&scene, &mut cache);
        let b = blocks[0].block.expect("declared");
        assert_eq!(b.heading_level, 9);
        assert_eq!(b.aria_level, Some(6));
    }

    /// ★★★★★ R1780 — **a client can see whether a declared alignment had room
    /// to act, and the wire's own numbers say so.**
    ///
    /// # Why this needed asserting
    ///
    /// A debt stood for 84 rounds saying a declared `TextAlign` "does nothing"
    /// and that a consumer "has no way to know". Measured at R1780, both halves
    /// were off. Alignment aligns within the width the node is given, so a box
    /// the size of its own text is a box with nowhere to move — that is the
    /// property behaving as specified, not failing. And this report already
    /// carries everything needed to see it: the block's `width`, and each
    /// line's `x` — which its own documentation calls the place "where BOTH the
    /// text indent and the alignment land" — with `advance` and
    /// `trailing_whitespace` beside it.
    ///
    /// What was missing is this: **nothing checked that those fields move.** A
    /// client author had the numbers and no evidence they were live, and a
    /// regression that froze `x` at zero would have been invisible on the wire
    /// while every paint gate stayed green.
    #[test]
    fn r1780_the_wire_shows_whether_an_alignment_had_room() {
        use pinion_core::style::TextAlign;

        let text = "centre me";
        let mut cache = LayoutCache::new();

        let line_x = |cache: &mut LayoutCache, box_w: u32, align: TextAlign| -> (f64, f64, u32) {
            let scene = Scene::Text(TextNode::styled(
                text.to_string(),
                Rect::new(0, 0, box_w, 20),
                TextStyle::new().with_align(align),
            ));
            let blocks = collect_blocks(&scene, cache);
            let block = blocks.first().expect("one text leaf");
            let line = *block.lines.first().expect("one line");
            (line.x, line.advance - line.trailing_whitespace, block.width)
        };

        // ★★ ONE declaration, two boxes — which is what this question actually
        // is, and the first draft got it wrong in a way worth keeping.
        //
        // That draft took its flush baseline from `TextAlign::Start`, which is
        // the DEFAULT and which this surface deliberately does not report: a
        // default nobody asked for is not a declaration. So the baseline call
        // came back empty and the test failed on its own premise. Comparing the
        // same declaration across two widths needs no baseline and is the
        // sentence in the test's name.
        let (wide_x, inked, wide_box) = line_x(&mut cache, 400, TextAlign::Center);
        assert!(
            inked > 0.0 && f64::from(wide_box) > inked + 2.0,
            "the premise: a box of {wide_box} holding {inked} of ink has slack",
        );
        assert!(
            wide_x > 1.0,
            "given room, the wire must SHOW where centring put the line, not \
             just that it was asked for: x {wide_x} in a box of {wide_box}",
        );
        // And by half the slack, or "it moved" would pass for any movement.
        let expected = (f64::from(wide_box) - inked) / 2.0;
        assert!(
            (wide_x - expected).abs() < 2.0,
            "and by half the slack: x {wide_x} against an expected {expected}",
        );

        // ★ The same declaration in a box with no room reports none — which is
        // the case a reader saw and called a broken property.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an inked width in px, asserted positive above"
        )]
        let tight_w = inked.ceil() as u32;
        let (tight_x, tight_inked, tight_box) = line_x(&mut cache, tight_w, TextAlign::Center);
        assert!(
            f64::from(tight_box) - tight_inked < 2.0,
            "the premise: this box has no slack — {tight_box} against {tight_inked}",
        );
        assert!(
            tight_x.abs() < 1.0,
            "with no slack the declaration cannot move anything, and the wire \
             reports exactly that rather than an alignment that did something: \
             x {tight_x}",
        );
    }
}
