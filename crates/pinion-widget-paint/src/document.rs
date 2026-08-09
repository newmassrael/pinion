//! R1551 §5.36 §5.40 — rich-text **document** composition: a sequence of
//! paragraphs, each with its own [`BlockFormat`].
//!
//! # Why a document is a column of blocks
//!
//! The toolkit's text document owns a private layout engine (text document
//! layout) that stacks blocks and applies their margins itself, which is why a
//! toolkit block's indent is invisible to the widget layout around it: the two
//! are different layout systems that meet only at a text edit's viewport.
//!
//! Here a block IS a scene node. Its format lowers to the node's margin
//! ([`TextNode::with_block`]), the ordinary flex pass stacks the column, and the result composes
//! with everything else in the tree — a paragraph can sit beside a widget,
//! inside a splitter, in a scroll — with no document-specific layout code at
//! all, on **both** backends. A text document can do none of that.
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
//! The marker is an ordinary [`TextNode`]. That is the whole reason the cell backend
//! needs no list code, the shape cache measures markers like any other text,
//! and `scene/text_lists` can publish where a bullet landed. The toolkit's text document
//! layout draws its unordered markers as an ellipse or a rectangle, so in the
//! toolkit a bullet is not text and none of that follows.

use pinion_core::composite_tag::DocumentTag;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, StyleRun, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BlockFormat, Border, BoxStyle, FlexDirection, GridPlacement, LayoutStyle, Size,
    SizeValue, TextAlign, TextIndent, TextStyle,
};
use pinion_core::text_list::{ListNumbering, ListPlacement, ListSpec, number_blocks};
use pinion_core::text_table::{CellPlacement, CellSpec, TableAddressing, TablePart, place_cells};
use std::ops::Range;

/// One paragraph of a document: its text, its block format, and its inline
/// styling.
///
/// The character style is `Option`: `None` inherits the document's base style, which
/// is the common case and keeps a plain paragraph a one-field construction.
/// That mirrors the toolkit, where a block's characters carry the document's
/// default text char format until something overrides them.
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
    /// R1560 — this paragraph is in a **table cell**: under what format, how
    /// far the cell reaches, and whether it opens a cell or continues the
    /// previous one.
    ///
    /// `None` (the default) is a paragraph outside any table. Membership and
    /// reach only — the author never states an address, because an address is
    /// not something a cell has; see
    /// [`pinion_core::text_table::place_cells`].
    ///
    /// Composes with [`Self::list`] rather than excluding it: a list inside a
    /// table cell is an ordinary document, and the numbering runs *per cell*
    /// so an item in one cell is not item 2 of a list that started in another.
    pub cell: Option<CellSpec>,
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
            cell: None,
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

    /// R1559 builder: this paragraph is an item of a list (the toolkit
    /// `createList` / `add`).
    #[must_use]
    pub fn in_list(mut self, spec: ListSpec) -> Self {
        self.list = Some(spec);
        self
    }

    /// R1560 builder: this paragraph is in a table cell (the toolkit
    /// `cellAt(...).firstCursorPosition()`).
    #[must_use]
    pub fn in_cell(mut self, spec: CellSpec) -> Self {
        self.cell = Some(spec);
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
    let cell_specs: Vec<Option<CellSpec>> = blocks.iter().map(|b| b.cell.clone()).collect();
    let addressing = place_cells(&cell_specs, |part| match part {
        TablePart::Table(k) => DocumentTag::table(tag, k),
        TablePart::Row(k, r) => DocumentTag::table_row(tag, k, r),
        TablePart::Cell(i) => DocumentTag::cell(tag, i),
    });

    let mut root: Vec<Scene> = Vec::new();
    // Lists are numbered per SEGMENT — see `fold_lists`. The counter runs
    // across segments so two lists in one document never share a tag, however
    // deeply the tables between them nest.
    let mut next_list = 0usize;
    let mut i = 0usize;
    while i < blocks.len() {
        let Some(first) = addressing.placements[i].as_ref() else {
            let end = run_end(&addressing, i, |p| p.is_none());
            root.extend(fold_lists(
                tag,
                base,
                blocks,
                i..end,
                &addressing,
                &mut next_list,
            ));
            i = end;
            continue;
        };
        let table_tag = first.table_tag.clone();
        let end = run_end(&addressing, i, |p| {
            p.is_some_and(|p| p.table_tag == table_tag)
        });
        root.push(table_node(
            tag,
            base,
            blocks,
            i..end,
            &addressing,
            &mut next_list,
        ));
        i = end;
    }

    ContainerNode::new(root)
        .with_tag(DocumentTag::document(tag))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch),
        )
}

/// The end of the maximal run starting at `start` whose placements all satisfy
/// `keep` — the one place the block sequence is cut into segments.
fn run_end(
    addressing: &TableAddressing,
    start: usize,
    keep: impl Fn(Option<&CellPlacement>) -> bool,
) -> usize {
    let mut end = start;
    while end < addressing.placements.len() && keep(addressing.placements[end].as_ref()) {
        end += 1;
    }
    end
}

/// Fold one **segment** of blocks — a stretch outside any table, or the blocks
/// of one cell — into paint nodes, resolving its list structure.
///
/// The numbering runs per segment rather than once over the document, and that
/// is a correctness property rather than an optimisation: a cell boundary ends
/// a list, so two items in different cells are two lists that each start at 1.
/// Numbering the document as one sequence would make the second cell's first
/// item read `2.`, which is exactly the class of error the derivation exists to
/// rule out. `next_list` carries the tag counter across segments so the lists
/// still have document-unique tags.
///
/// A document with no tables is one segment, so this produces the pre-R1560
/// tree unchanged.
fn fold_lists(
    tag: &str,
    base: &TextStyle,
    blocks: &[TextBlock],
    range: Range<usize>,
    addressing: &TableAddressing,
    next_list: &mut usize,
) -> Vec<Scene> {
    let start = range.start;
    let specs: Vec<Option<ListSpec>> = blocks[range.clone()]
        .iter()
        .map(|b| b.list.clone())
        .collect();
    let base_index = *next_list;
    let numbering = number_blocks(&specs, |k| DocumentTag::list(tag, base_index + k));
    *next_list += numbering.runs.len();

    let mut out: Vec<Scene> = Vec::new();
    let mut stack: Vec<OpenList> = Vec::new();
    for (offset, block) in blocks[range].iter().enumerate() {
        let i = start + offset;
        let style = block.style.clone().unwrap_or_else(|| base.clone());
        let placement = numbering.placements.get(offset).and_then(Option::as_ref);
        let mut text = TextNode::styled(block.text.clone(), Rect::new(0, 0, 0, 0), style.clone())
            .with_runs(block.runs.clone())
            .with_block(block.format)
            .with_tag(DocumentTag::block(tag, i));
        if let Some(cell) = addressing.placements[i].as_ref() {
            // The paragraph carries its cell's address for the reason it
            // carries its list placement: the a11y pass and the §7 census read
            // the painted scene, and a grid area cannot be read back as the
            // allocation that produced it.
            text = text.with_cell_placement(cell.clone());
        }
        let Some(placement) = placement else {
            // An ordinary paragraph ends every open list, which is what
            // `number_blocks` already decided; closing here keeps the painted
            // nesting and the derived numbering one structure.
            close_lists_to(&mut stack, &mut out, 0);
            out.push(Scene::Text(text));
            continue;
        };
        open_lists_for(&mut stack, &mut out, &numbering, &placement.list_tag);
        text = text
            .with_list_placement(placement.clone())
            // The paragraph takes what the marker gutter leaves. Stated on the
            // paragraph rather than as a width on the marker, so a wide marker
            // (`MMMCMXCIX.`) narrows the text instead of overflowing the row.
            .map_layout(|l| l.with_flex_grow(1.0));
        let row = item_row(tag, i, placement, &style, Scene::Text(text));
        push_into(&mut stack, &mut out, row);
    }
    close_lists_to(&mut stack, &mut out, 0);
    out
}

/// One table: a CSS Grid whose column tracks are the format's, holding a band
/// per row and a box per cell.
///
/// # Why the grid, and why the rows are boxes rather than parents
///
/// A column of flex rows measures each row on its own, so the columns of two
/// rows line up only if something states their width; a grid sizes each track
/// once against every cell in it, which is what makes a table's columns agree
/// when nothing declares them. It is also the only shape in which a `rowspan`
/// is expressible: a cell that covers two rows cannot be a child of one of
/// them.
///
/// So the rows cannot own the cells, and they are emitted as full-width bands
/// *behind* them — the CSS-Grid-table idiom that HTML spells `tr { display:
/// contents }`, minus the indirection. A band is what carries the WAI-ARIA
/// `row`'s bounds, what a caller measures to ask how tall a row is, and what a
/// future stripe or header tint is painted on. It states no fill of its own and
/// is pointer-transparent, so it changes neither the pixels nor the hit test.
fn table_node(
    tag: &str,
    base: &TextStyle,
    blocks: &[TextBlock],
    range: Range<usize>,
    addressing: &TableAddressing,
    next_list: &mut usize,
) -> Scene {
    let Some(first) = addressing.placements[range.start].as_ref() else {
        // Unreachable: the caller cut this range on `Some`. Emitting the
        // blocks loose rather than panicking keeps a malformed addressing a
        // rendering fault instead of a crash.
        return Scene::Container(ContainerNode::new(fold_lists(
            tag, base, blocks, range, addressing, next_list,
        )));
    };
    let format = first.format.clone();
    let table_tag = first.table_tag.clone();
    let rows = first.row_count;
    let columns = format.column_count();

    let mut children: Vec<Scene> = Vec::new();
    for row in 0..rows {
        let row_tag = addressing.run(&table_tag).map_or_else(
            || first.row_tag.clone(),
            |run| DocumentTag::table_row(tag, run.index, row),
        );
        children.push(Scene::Box(
            BoxNode::new(Rect::new(0, 0, 0, 0), BoxStyle::default())
                .with_tag(row_tag)
                .with_layout(
                    LayoutStyle::new()
                        .with_grid_row(GridPlacement::at(line(row)))
                        .with_grid_column(GridPlacement::spanning(1, columns))
                        .with_pointer_transparent(true),
                ),
        ));
    }

    let mut i = range.start;
    while i < range.end {
        let Some(placement) = addressing.placements[i].as_ref() else {
            break;
        };
        // A cell is its opening block plus every continuation after it.
        let mut end = i + 1;
        while end < range.end
            && addressing.placements[end]
                .as_ref()
                .is_some_and(|p| !p.opens_cell)
        {
            end += 1;
        }
        children.push(cell_node(
            tag,
            base,
            blocks,
            i..end,
            placement,
            addressing,
            next_list,
        ));
        i = end;
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(table_tag)
            .with_layout(
                LayoutStyle::new()
                    .grid_columns(format.tracks())
                    .with_gap(format.cell_spacing_px),
            ),
    )
}

/// One cell: its blocks, in a box placed at the address the allocation gave it.
fn cell_node(
    tag: &str,
    base: &TextStyle,
    blocks: &[TextBlock],
    range: Range<usize>,
    placement: &CellPlacement,
    addressing: &TableAddressing,
    next_list: &mut usize,
) -> Scene {
    let format = &placement.format;
    let mut style = BoxStyle::default();
    if format.border_px > 0 {
        style = style.with_border(Border::new(format.border_color, format.border_px));
    }
    let padding = format.cell_padding_px;
    Scene::Container(
        ContainerNode::new(fold_lists(tag, base, blocks, range, addressing, next_list))
            .with_tag(placement.cell_tag.clone())
            .with_style(style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_grid_row(GridPlacement::spanning(
                        line(placement.row),
                        placement.row_span,
                    ))
                    .with_grid_column(GridPlacement::spanning(
                        line(placement.column),
                        placement.column_span,
                    ))
                    .with_padding(Rect::new(padding, padding, padding, padding)),
            ),
    )
}

/// The CSS grid **line** a 0-based track index starts at.
///
/// One function because the off-by-one is the seam between two numbering
/// conventions — a table's addresses are 0-based (the toolkit `row()`)
/// and CSS's grid lines are 1-based — and a conversion spelled at each of the
/// four call sites is a conversion that can be forgotten at one of them.
fn line(track: u32) -> u16 {
    u16::try_from(track.saturating_add(1)).unwrap_or(u16::MAX)
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
/// be measured, copied, shaped, painted by the cell backend, and read back
/// over the wire. The toolkit draws its unordered markers as an ellipse or a
/// rectangle inside text document layout, so none of that is possible there.
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
            // The item's own character style, as the toolkit's marker takes
            // the block's char format — so a marker matches the size and
            // colour of the text it belongs to. Two paragraph-level fields are
            // dropped: the alignment, because a marker is end-aligned in its
            // gutter by definition, and the text indent, which would push the
            // marker inside its own box.
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
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode, TextRole};
    use pinion_core::style::{
        BlockFormat, Color, Display, GridPlacement, GridTrack, SizeValue, TextAlign, TextIndent,
        TextStyle,
    };
    use pinion_core::text_list::{ListFormat, ListSpec, ListStyle};
    use pinion_core::text_table::{CellSpec, TableFormat};

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
        use pinion_core::scene::TextNode;
        use pinion_core::style::{AlignItems, FlexDirection, LayoutStyle};
        let fmt = BlockFormat::new().with_indent(32).with_spacing(4, 6);
        let extra = LayoutStyle::new()
            .flex(FlexDirection::Column)
            .with_align_items(AlignItems::Stretch);
        let a = TextNode::new("q", Rect::new(0, 0, 0, 0))
            .with_block(fmt)
            .with_layout(extra.clone());
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
            // R1560 — a table's row bands are `Scene::Box`. Reporting only
            // containers and text made the first draft of
            // `r1560_a_table_is_a_grid_of_row_bands_and_cell_boxes` pass
            // against a tree that had no bands at all.
            Scene::Box(b) => {
                if let Some(tag) = b.tag.as_deref() {
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

    // ── R1560: tables ─────────────────────────────────────────────────────

    fn cell(text: &str, format: &TableFormat) -> TextBlock {
        TextBlock::new(text).in_cell(CellSpec::new(format.clone()))
    }

    /// A table becomes a real subtree: a grid container holding one band per
    /// row and one box per cell, each box placed at the address the allocation
    /// derived. That structure is what gives a table a box, a cell a tag, and
    /// the columns somewhere to agree.
    #[test]
    fn r1560_a_table_is_a_grid_of_row_bands_and_cell_boxes() {
        let format = TableFormat::new(2);
        let blocks = vec![
            TextBlock::new("Intro."),
            cell("a", &format),
            cell("b", &format),
            cell("c", &format),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let mut found = Vec::new();
        tags(&scene, &mut found);
        assert_eq!(
            found,
            [
                "doc_doc",
                "doc_blk0",
                "doc_tbl0",
                "doc_tbl0r0",
                "doc_tbl0r1",
                "doc_cel1",
                "doc_blk1",
                "doc_cel2",
                "doc_blk2",
                "doc_cel3",
                "doc_blk3",
            ],
            "the bands come first, so the cells paint over them",
        );
        let table = find_container(&scene, "doc_tbl0").expect("the table");
        assert_eq!(table.layout.display, Display::Grid);
        assert_eq!(
            table.layout.grid_template_columns,
            [GridTrack::Auto, GridTrack::Auto]
        );
    }

    /// The derived address IS the grid placement — one derivation reaching the
    /// layout engine, with CSS's 1-based lines converted in exactly one place.
    #[test]
    fn r1560_the_derived_address_is_the_grid_placement() {
        let format = TableFormat::new(2);
        let blocks = vec![cell("a", &format), cell("b", &format), cell("c", &format)];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let third = find_container(&scene, "doc_cel2").expect("the third cell");
        assert_eq!(third.layout.grid_row, Some(GridPlacement::spanning(2, 1)));
        assert_eq!(
            third.layout.grid_column,
            Some(GridPlacement::spanning(1, 1))
        );
        let placement = find_text(&scene, "doc_blk2")
            .expect("the paragraph")
            .cell
            .clone()
            .expect("addressed");
        assert_eq!((placement.row, placement.column), (1, 0));
        assert_eq!(
            placement.cell_tag, "doc_cel2",
            "the painted box and the published address are one object",
        );
    }

    /// A spanning cell covers the tracks it was given, and a span that did not
    /// fit reaches the layout engine already clamped — so the painted box and
    /// the published span cannot disagree.
    #[test]
    fn r1560_a_span_reaches_the_layout_as_the_clamped_one() {
        let format = TableFormat::new(3);
        let blocks = vec![
            TextBlock::new("wide").in_cell(CellSpec::new(format.clone()).spanning_columns(2)),
            TextBlock::new("tall").in_cell(CellSpec::new(format.clone()).spanning_rows(2)),
            TextBlock::new("over").in_cell(CellSpec::new(format).spanning_columns(9)),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let wide = find_container(&scene, "doc_cel0").expect("the wide cell");
        assert_eq!(wide.layout.grid_column, Some(GridPlacement::spanning(1, 2)));
        let tall = find_container(&scene, "doc_cel1").expect("the tall cell");
        assert_eq!(tall.layout.grid_row, Some(GridPlacement::spanning(1, 2)));
        let over = find_container(&scene, "doc_cel2").expect("the third cell");
        assert_eq!(
            over.layout.grid_column,
            Some(GridPlacement::spanning(1, 2)),
            "row 1 starts at column 0 and the tall cell holds column 2",
        );
        let published = find_text(&scene, "doc_blk2")
            .expect("the paragraph")
            .cell
            .clone()
            .expect("addressed");
        assert_eq!(published.column_span, 2);
        assert_eq!(published.declared_column_span, 9);
        assert!(published.clamped());
    }

    /// A multi-block cell is one box holding both paragraphs — the toolkit's
    /// cell is a frame of blocks, and this is the flat-sequence spelling of
    /// that.
    #[test]
    fn r1560_a_continuation_block_joins_the_same_cell_box() {
        let format = TableFormat::new(2);
        let blocks = vec![
            cell("first para", &format),
            TextBlock::new("second para").in_cell(CellSpec::new(format.clone()).continued()),
            cell("next cell", &format),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        let box_ = find_container(&scene, "doc_cel0").expect("the first cell");
        assert_eq!(box_.children.len(), 2, "one box, two paragraphs");
        assert!(
            find_container(&scene, "doc_cel1").is_none(),
            "no second box"
        );
        assert_eq!(
            find_container(&scene, "doc_cel2")
                .expect("the next cell")
                .layout
                .grid_column,
            Some(GridPlacement::spanning(2, 1)),
        );
    }

    /// A list inside a cell is its own list: the numbering restarts per cell,
    /// which is the property a document-wide numbering would get wrong.
    #[test]
    fn r1560_a_list_in_a_cell_is_numbered_within_that_cell() {
        let format = TableFormat::new(2);
        let numbered = |text: &str| {
            TextBlock::new(text)
                .in_cell(CellSpec::new(format.clone()))
                .in_list(ListSpec::new(ListFormat::new(ListStyle::Decimal)))
        };
        let follow = |text: &str| {
            TextBlock::new(text)
                .in_cell(CellSpec::new(format.clone()).continued())
                .in_list(ListSpec::new(ListFormat::new(ListStyle::Decimal)))
        };
        let blocks = vec![
            numbered("left one"),
            follow("left two"),
            numbered("right one"),
        ];
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &blocks));
        assert_eq!(find_text(&scene, "doc_mrk0").expect("marker").content, "1.");
        assert_eq!(find_text(&scene, "doc_mrk1").expect("marker").content, "2.");
        assert_eq!(
            find_text(&scene, "doc_mrk2").expect("marker").content,
            "1.",
            "the next cell's list starts again",
        );
        let left = find_text(&scene, "doc_blk1")
            .expect("paragraph")
            .list
            .clone()
            .expect("an item");
        let right = find_text(&scene, "doc_blk2")
            .expect("paragraph")
            .list
            .clone()
            .expect("an item");
        assert_ne!(left.list_tag, right.list_tag, "two lists, two tags");
        assert_eq!(left.count, 2);
        assert_eq!(right.count, 1);
    }

    /// The cell's declared metrics reach its box: the padding is the box's
    /// padding and the border is the box's border.
    #[test]
    fn r1560_the_format_metrics_reach_the_cell_box() {
        let format = TableFormat::new(1)
            .with_metrics(7, 3)
            .with_border(2, Color::rgb(0x11, 0x22, 0x33));
        let scene = Scene::Container(view_document(
            "doc",
            &TextStyle::new(),
            &[cell("only", &format)],
        ));
        let cell_box = find_container(&scene, "doc_cel0").expect("the cell");
        assert_eq!(cell_box.layout.padding, Rect::new(7, 7, 7, 7));
        let border = cell_box.style.border.expect("a rule");
        assert_eq!(border.width, 2);
        assert_eq!(border.color, Color::rgb(0x11, 0x22, 0x33));
        assert_eq!(
            find_container(&scene, "doc_tbl0")
                .expect("the table")
                .layout
                .gap,
            3,
            "cell spacing is the grid gap",
        );
    }

    /// The negative control: a document with no cell membership paints exactly
    /// what it did before this round — no grid, no bands, no cell boxes.
    #[test]
    fn r1560_a_document_with_no_cells_paints_no_table_machinery() {
        let scene = Scene::Container(view_document("doc", &TextStyle::new(), &doc()));
        let mut found = Vec::new();
        tags(&scene, &mut found);
        assert_eq!(found, ["doc_doc", "doc_blk0", "doc_blk1", "doc_blk2"]);
        let root = find_container(&scene, "doc_doc").expect("the document");
        assert_eq!(root.layout.display, Display::Flex);
        assert!(root.layout.grid_template_columns.is_empty());
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
