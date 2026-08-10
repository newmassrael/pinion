//! R1344 §5.41 §5.36 — cell-grid text layout: the TUI's measure/paint SSOT.
//!
//! Resolves §5.41's open substrate decision (*"`ColorBrush` RGBA → ANSI color
//! 매핑 + **`TextAlign` wrap policy** = R51.109 substrate 결정"*), which had stayed
//! open since the TUI backend landed. Before R1344 the paint walker hand-rolled a
//! grapheme loop that never wrapped and never read `TextNode.rect.w`, so a
//! `Scene::Text` that the Vello backend wrapped into a paragraph rendered on the
//! terminal as one truncated row — a §2 #6 (*one scene, two render dispatch
//! paths*) divergence.
//!
//! ## Line breaking is not shaping
//!
//! The reason the gap survived so long is a conflation worth naming: a terminal
//! is a uniform grid, so **shaping** (glyph selection, kerning, ligatures) is
//! genuinely meaningless there — but **line breaking** is not shaping. UAX #14
//! break opportunities are a property of the text, not the font. So the cell
//! backend reuses pinion's own UAX #14 breaker
//! ([`pinion_text_unicode::wrap_paragraph_with_line_budget`], §5.37.7) and
//! supplies the only genuinely backend-specific inputs: how wide a segment is,
//! in cells, and how much room each line has.
//!
//! ### Honest scope: this is not yet ONE breaker across both backends
//!
//! The shared breaker is shared with the **§5.37 self-hosted** text path, not
//! with what Vello runs today. Production Vello wraps through **parley**: the
//! §5.37 measure arm is opt-in (`PINION_TEXT_ENGINE`), and even when enabled
//! `self_hosted_text_eligible` declines any content with a hard break and
//! `single_line_overflows` declines anything that would soft-wrap — so the
//! §5.37 engine never wraps in production at all.
//!
//! So pinion currently has two production line breakers — parley for pixels,
//! this one for cells — and their break points can differ for the same
//! `Scene::Text` (parley measures real advances; this measures cell columns).
//! That is a **narrower** §2 #6 gap than the one R1344 closed (before it, the
//! TUI did not wrap at all and text left its box), and it is the unavoidable
//! floor until §5.37 becomes the production pixel path: a terminal has no fonts
//! to measure with, so a cell backend cannot defer to parley. What this module
//! must NOT do is claim parity it does not have — the alternative to reusing
//! pinion's breaker was hand-rolling a third one, which is what the pre-R1344
//! grapheme loop effectively was.
//!
//! ## R1551 — the same module PLACES the line, not only breaks it
//!
//! A paragraph's own format decides where each of its lines starts: CSS
//! `text-indent` moves one line (or all but one, with `hanging`), and
//! `text-align` distributes whatever the line did not use. Both are one
//! question — "which column does this line begin at" — so
//! [`CellTextLayout::place`] answers it once, and the indent is asked for
//! inside the breaker's per-line budget callback because it is *also* a
//! breaking input: an indented line has less room, so where it breaks depends
//! on it. Splitting the two would be two derivations of one CSS rule that must
//! agree about which lines are selected, and the selection is not recoverable
//! from the ranges — a line after a soft wrap and a line after a hard break
//! look identical there.
//!
//! [`CellTextLayout::wrap`] survives as the `TextStyle::new()` case of
//! `place`, so a caller with no paragraph style gets the pre-R1551 behaviour by
//! construction rather than through a second function.
//!
//! ## One SSOT, two callers
//!
//! [`CellTextLayout::place`] is called from both:
//!
//! * the **measure** pass — `impl TextMeasure`, consulted by
//!   `compute_layout_with_text_measure` to size a `Scene::Text` node; and
//! * the **paint** pass — [`crate::paint`], to place each row.
//!
//! They must agree exactly, or a box sized for N rows gets N±1 rows of text. The
//! R1070 Vello precedent learned this the hard way (see `TextMeasure`'s rustdoc
//! on sharing the `single_line_overflows` SSOT between its measure and paint
//! arms); here the coupling is structural — there is one `place` and both call
//! it.
//!
//! ## Control characters never reach a cell
//!
//! [`pinion_text_unicode::wrap_paragraph_with_line_budget`] emits line ranges that
//! span *through* their terminating break codepoint. A glyph rasterizer shapes
//! that harmlessly, but writing `U+000A` into a terminal cell emits a raw line
//! feed into the output stream mid-frame — the cursor moves and the frame
//! corrupts. So every line is trimmed
//! ([`pinion_text_unicode::trim_trailing_break`]) and [`cell_width`] scores every
//! control character as zero width, which the paint walker mirrors by skipping
//! them. `unicode-width` reports width **1** for C0 controls (they are not
//! zero-width in its tables), so this is an explicit policy, not a fallout of the
//! width table.

use pinion_core::Scene;
use pinion_core::cell_metric::CellMetric;
use pinion_core::scene::StyleRun;
use pinion_core::style::{TextAlign, TextIndent, TextStyle};
use pinion_runtime::layout::{TextBox, TextMeasure};
use pinion_runtime::{LayoutCache, LayoutPass, compute_layout_with_text_measure};
use pinion_text_unicode::{LineRange, trim_trailing_break, wrap_paragraph_with_line_budget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// R1344 §5.41 R968 — the cell metric the whole TUI backend resolves against.
///
/// One home for the 8×16 baseline: the layout pass budgets boxes with it and
/// the paint walker floors `rect` back to cells with it, so the two MUST agree
/// — a divergence here would size a box for N cells and paint M, the same
/// measure/paint split this module closes one level down at the wrap. Pre-R1344
/// `paint.rs` and `input.rs` each named `CellMetric::DEFAULT`; R1344's layout
/// pass would have added three more, so it gets a single declaration instead.
///
/// A per-node metric (R968 `Scene::TextGrid` carry) threads from here when it
/// lands.
pub const CELL: CellMetric = CellMetric::DEFAULT;

/// Display width of `text` in terminal cells.
///
/// Grapheme-cluster based (`unicode-width` on whole clusters, matching the paint
/// walker's advance): CJK / fullwidth Latin = 2, narrow ASCII = 1, combining
/// marks and ZWJ = 0.
///
/// **Control characters score 0** — `unicode-width` gives C0 controls width 1,
/// but a terminal cell cannot hold one (see the module docs), so the layout must
/// not budget space the paint walker will not fill. Line-break controls are
/// consumed by the breaker itself; any other control (a stray `\t`, C1) is
/// skipped by both this function and [`crate::paint`], keeping measure and paint
/// in lockstep.
#[must_use]
pub fn cell_width(text: &str) -> usize {
    text.graphemes(true).map(grapheme_cells).sum()
}

/// Cells spanned by one grapheme cluster — the shared advance rule for
/// [`cell_width`] (measure) and [`crate::paint`] (paint).
#[must_use]
pub fn grapheme_cells(grapheme: &str) -> usize {
    if is_unpaintable_control(grapheme) {
        return 0;
    }
    grapheme.width()
}

/// Whether a grapheme cluster is a control character no cell can hold.
///
/// A cluster whose first `char` is a C0/C1 control and which carries no other
/// printable codepoint. Checked on the first char rather than the whole cluster
/// because a combining sequence never begins with a control.
fn is_unpaintable_control(grapheme: &str) -> bool {
    grapheme.chars().next().is_some_and(char::is_control)
}

/// Cell-grid text layout — the TUI's line-breaking SSOT.
///
/// Carries the [`CellMetric`] so the px ↔ cell conversion the measure pass owes
/// taffy (which speaks logical pixels) stays in one place.
#[derive(Debug, Clone, Copy)]
pub struct CellTextLayout {
    metric: CellMetric,
}

impl Default for CellTextLayout {
    fn default() -> Self {
        Self::new(CELL)
    }
}

impl CellTextLayout {
    /// Construct a layout over `metric`.
    #[must_use]
    pub const fn new(metric: CellMetric) -> Self {
        Self { metric }
    }

    /// R1551 §5.36 — this style's CSS `text-indent` in cells, signed.
    ///
    /// The magnitude floors to whole cells the way every other px→cell
    /// conversion here does (a 2.5-cell indent indents 2), and the sign is
    /// applied afterwards so an outdent of the same length is the same number
    /// of cells. Flooring the *signed* value instead would make a −20px indent
    /// on an 8px cell come out at −3 where +20px comes out at +2.
    #[must_use]
    pub const fn indent_cols(self, indent: TextIndent) -> i32 {
        let cell_w = self.metric.cell_w();
        if cell_w == 0 {
            return 0;
        }
        let magnitude = indent.amount_px.unsigned_abs() / cell_w;
        #[allow(
            clippy::cast_possible_wrap,
            reason = "an indent is a UI length; its cell count is far below i32::MAX"
        )]
        let cells = magnitude as i32;
        if indent.amount_px < 0 { -cells } else { cells }
    }

    /// R1551 §5.36 — break `content` to `max_cols` **and place each line in the
    /// box** according to the paragraph-level fields of `style`: CSS
    /// `text-indent` and `text-align`.
    ///
    /// Placement and breaking are one operation because the indent is both: it
    /// narrows the line it applies to (so it changes where the break falls) and
    /// it shifts that line (so it changes where the glyphs go). Computing them
    /// apart would be two derivations of one CSS rule that must agree about
    /// which lines are selected — and the selection is not recoverable from the
    /// ranges, because a line after a soft wrap and a line after a hard break
    /// look identical there. So the rule
    /// ([`TextIndent::indents_line`](pinion_core::style::TextIndent::indents_line))
    /// is asked once, inside the breaker's own per-line budget callback, and
    /// its answer is recorded for the placement pass.
    ///
    /// Alignment offsets are measured against the line's **trailing-trimmed**
    /// width, mirroring parley's `free_space` (which adds
    /// `trailing_whitespace` back). Without that an end-aligned line would sit
    /// short of the edge by however many spaces the greedy breaker left on it.
    #[must_use]
    pub fn place(self, content: &str, max_cols: u32, style: &TextStyle) -> Vec<CellLine> {
        let indent = self.indent_cols(style.text_indent);
        let box_cols = i64::from(max_cols);
        // One entry per line, in line order — the breaker calls the budget
        // exactly once per line it produces, which is what makes this parallel
        // to `lines` by construction rather than by a second walk.
        let mut indented: Vec<bool> = Vec::new();
        let lines = wrap_paragraph_with_line_budget(
            content,
            |seg| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a segment's cell width is bounded by the text length"
                )]
                let w = cell_width(seg) as f32;
                w
            },
            |ctx| {
                let is_indented = style
                    .text_indent
                    .indents_line(ctx.is_block_start, ctx.is_scope_start);
                indented.push(is_indented);
                let budget = if is_indented {
                    box_cols - i64::from(indent)
                } else {
                    box_cols
                };
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a cell budget is a terminal column count"
                )]
                let w = budget.max(0) as f32;
                w
            },
        );
        let last = lines.len().saturating_sub(1);
        lines
            .iter()
            .enumerate()
            .map(|(i, range)| {
                let indent_col = if indented.get(i).copied().unwrap_or(false) {
                    indent
                } else {
                    0
                };
                let raw = &content[range.start..range.end];
                let text = trim_trailing_break(raw);
                let measured = i64::try_from(cell_width(text.trim_end())).unwrap_or(i64::MAX);
                let avail = (box_cols - i64::from(indent_col)).max(0);
                let free = (avail - measured).max(0);
                // A line that ends the paragraph, or ends at a hard break, is
                // never justified — CSS and parley agree, and a stretched last
                // line is the classic justification artefact.
                let ends_scope = i == last || raw.len() != text.len();
                let (align_col, gap_pad) =
                    Self::distribute(style.text_align, free, text, ends_scope);
                CellLine {
                    range: *range,
                    indent_col,
                    align_col,
                    gap_pad,
                }
            })
            .collect()
    }

    /// R1551 — give a line's `free` cells to its alignment: an offset for
    /// start / centre / end, inter-word padding for justify.
    fn distribute(align: TextAlign, free: i64, text: &str, ends_scope: bool) -> (i32, GapPad) {
        let offset = |v: i64| i32::try_from(v).unwrap_or(i32::MAX);
        match align {
            TextAlign::Center => (offset(free / 2), GapPad::NONE),
            TextAlign::End => (offset(free), GapPad::NONE),
            TextAlign::Justify if !ends_scope => {
                let gaps = i64::from(justify_gaps(text));
                if gaps == 0 || free == 0 {
                    (0, GapPad::NONE)
                } else {
                    (
                        0,
                        GapPad {
                            each: u32::try_from(free / gaps).unwrap_or(u32::MAX),
                            leading_extra: u32::try_from(free % gaps).unwrap_or(0),
                        },
                    )
                }
            }
            // `Start`, and `Justify` on a line that ends its scope — CSS
            // start-aligns the last line of a justified paragraph.
            _ => (0, GapPad::NONE),
        }
    }

    /// Wrap `content` into lines no wider than `max_cols` cells — **the SSOT**
    /// both the measure pass and the paint walker resolve against.
    ///
    /// `max_cols == 0` (an unresolved / zero-width box) still yields one line per
    /// mandatory break rather than an empty layout: a zero budget makes every
    /// segment overflow, and the breaker's overflow rule emits each on its own
    /// line. The paint walker then clips them away horizontally. This keeps
    /// "nothing fits" and "nothing to draw" distinct — the row count still
    /// reflects the text's hard structure.
    ///
    /// Returns byte ranges into `content`, contiguous and covering it. Each range
    /// may include a trailing break codepoint; use [`Self::line_text`] to read
    /// the printable slice.
    /// R1551 — the un-placed break, for callers with no paragraph style: the
    /// `TextStyle::new()` case of [`Self::place`], not a second breaker. The
    /// CSS initial values (no indent, start alignment) place every line at
    /// column 0, so this is exactly the pre-R1551 behaviour and stays so by
    /// construction rather than by two functions agreeing.
    #[must_use]
    pub fn wrap(self, content: &str, max_cols: u32) -> Vec<LineRange> {
        self.place(content, max_cols, &TextStyle::new())
            .into_iter()
            .map(|l| l.range)
            .collect()
    }

    /// The printable text of `line` within `content` — the trailing UAX #14
    /// break codepoint removed so no control byte reaches a cell.
    #[must_use]
    pub fn line_text(self, content: &str, line: LineRange) -> &str {
        trim_trailing_break(&content[line.start..line.end])
    }

    /// Wrap `content` against a **pixel** box width, the unit taffy and
    /// `TextNode.rect` speak. The px budget floors to whole cells: a box 2.5
    /// cells wide holds 2.
    #[must_use]
    pub fn wrap_px(self, content: &str, max_width_px: u32) -> Vec<LineRange> {
        self.wrap(content, max_width_px / self.metric.cell_w())
    }

    /// R1551 — [`Self::place`] against a **pixel** box width, the unit taffy and
    /// `TextNode.rect` speak.
    #[must_use]
    pub fn place_px(self, content: &str, max_width_px: u32, style: &TextStyle) -> Vec<CellLine> {
        self.place(content, max_width_px / self.metric.cell_w(), style)
    }

    /// The laid-out box for `content` in cells: `(cols, rows)`.
    ///
    /// `cols` is the widest printable line (never more than `max_cols` unless a
    /// single unbreakable segment overflows — the breaker's documented overflow
    /// rule, which the paint walker clips). `rows` is the line count.
    ///
    /// R1551 — a line's own **indent** counts toward the measured width (an
    /// indented line does occupy those cells), while its **alignment** does
    /// not: alignment distributes space the box already has, so letting it
    /// widen the box would make an end-aligned paragraph's intrinsic size
    /// depend on the box it is being measured for. That is why [`CellLine`]
    /// keeps the two offsets apart instead of storing only their sum.
    #[must_use]
    pub fn measure_cells(self, content: &str, max_cols: u32, style: &TextStyle) -> (u32, u32) {
        let lines = self.place(content, max_cols, style);
        let cols = lines
            .iter()
            .map(|l| {
                let w =
                    i64::try_from(cell_width(self.line_text(content, l.range))).unwrap_or(i64::MAX);
                (i64::from(l.indent_col) + w).max(0)
            })
            .max()
            .unwrap_or(0);
        (
            u32::try_from(cols).unwrap_or(u32::MAX),
            u32::try_from(lines.len()).unwrap_or(u32::MAX),
        )
    }
}

/// R1551 §5.36 — one wrapped line, **placed** in its box by the paragraph-level
/// style fields.
///
/// The two offsets stay separate because they answer different questions.
/// `indent_col` is content — an indented line occupies those cells, so it
/// belongs to the paragraph's intrinsic width. `align_col` is distribution —
/// it hands the line space the box already had, so it must not feed back into
/// the box's size. Storing only [`Self::start_col`] would collapse the two and
/// make an end-aligned paragraph's measured width grow with the box measuring
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellLine {
    /// Byte range into the source content, as [`CellTextLayout::wrap`] returns.
    pub range: LineRange,
    /// Signed cells this line is indented by CSS `text-indent`. Negative means
    /// it protrudes past the box's start edge, which is what a negative indent
    /// means (and what parley does on the pixel path).
    pub indent_col: i32,
    /// Cells of the line's unused width handed to it by CSS `text-align`
    /// (`0` for `Start`, and for `Justify`, which pads gaps instead).
    pub align_col: i32,
    /// Extra cells inserted at this line's inter-word gaps (CSS `justify`).
    pub gap_pad: GapPad,
}

impl CellLine {
    /// The column this line's first cell occupies, relative to the box's start
    /// edge: indent plus alignment.
    #[must_use]
    pub const fn start_col(self) -> i32 {
        self.indent_col + self.align_col
    }
}

/// R1551 §5.36 — how a justified line's leftover cells are spread across its
/// inter-word gaps.
///
/// A cell grid has no sub-cell positions, so the leftover rarely divides
/// evenly. `each` is the share every gap gets; `leading_extra` is how many of
/// the *leading* gaps get one more. Front-loading the remainder is a choice a
/// pixel backend does not have to make (parley adds a fractional advance to
/// every space), and it is the one that keeps the line's right edge exactly on
/// the box edge, which is the whole point of justifying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GapPad {
    /// Cells added to every inter-word gap.
    pub each: u32,
    /// How many of the leading gaps get one extra cell.
    pub leading_extra: u32,
}

impl GapPad {
    /// No padding — every alignment but `Justify`, and `Justify` on a line that
    /// ends its paragraph.
    pub const NONE: Self = Self {
        each: 0,
        leading_extra: 0,
    };

    /// Extra cells for the `n`-th inter-word gap of the line (0-based).
    #[must_use]
    pub const fn for_gap(self, n: u32) -> u32 {
        if n < self.leading_extra {
            self.each + 1
        } else {
            self.each
        }
    }

    /// Whether this pad moves anything.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.each == 0 && self.leading_extra == 0
    }
}

/// R1551 — inter-word gaps a justified line can stretch.
///
/// Counts runs of `U+0020`, not individual spaces: a double space is one gap,
/// so justification does not silently widen it twice. Trailing whitespace is
/// excluded by the caller (it hangs past the line's measured edge), and no
/// other codepoint counts — `U+00A0` is a *non-breaking* space, and stretching
/// it would defeat the reason an author typed it.
fn justify_gaps(text: &str) -> u32 {
    let mut gaps = 0u32;
    let mut in_gap = false;
    for c in text.trim_end().chars() {
        if c == ' ' {
            if !in_gap {
                gaps = gaps.saturating_add(1);
                in_gap = true;
            }
        } else {
            in_gap = false;
        }
    }
    gaps
}

/// Resolve `scene` for a `cols`×`rows` terminal — **the TUI's one layout
/// entry point**.
///
/// Converts the terminal's cell dimensions to the logical pixels taffy and
/// `Scene.rect` speak, then runs the shared layout pass against the cell text
/// measure. Returns taffy's scroll-dirty flag (see
/// [`crate::ShellCoreTui::compute_paint_scene`] for the re-pass it drives).
///
/// Every paint path must resolve a scene the same way, so both callers —
/// `ShellCoreTui::compute_paint_scene` (the live shell) and
/// [`crate::render_one_frame`] (the TTY-less helper) — route through here
/// rather than each spelling out the conversion. That is not cosmetic: before
/// R1344 the TUI had exactly this kind of second entry point, and
/// `render_one_frame` painted a *raw* `V::view` result while the shell did
/// something else. A scene laid out one way and painted another is the bug
/// class this round exists to close, so the two paths share one function
/// instead of one convention.
pub fn layout_for_terminal(
    scene: &mut Scene,
    cols: u16,
    rows: u16,
    cache: &mut LayoutCache,
) -> LayoutPass {
    layout_for_viewport_px(
        scene,
        u32::from(cols) * CELL.cell_w(),
        u32::from(rows) * CELL.cell_h(),
        cache,
    )
}

/// Resolve `scene` against a viewport given in **logical pixels** — the unit
/// taffy, `Scene.rect` and the §5.12 RPC surface all speak.
///
/// [`layout_for_terminal`] is the cell-dimensioned convenience over this; the
/// RPC paint producer (`scene/layout {viewport}`) calls this directly because
/// its hypothetical viewport already arrives in px.
pub fn layout_for_viewport_px(
    scene: &mut Scene,
    w_px: u32,
    h_px: u32,
    cache: &mut LayoutCache,
) -> LayoutPass {
    compute_layout_with_text_measure(scene, cache, w_px, h_px, Some(&CellTextLayout::new(CELL)))
}

/// R1344 §5.41 — the TUI's measure arm.
///
/// Always returns `Some`: the cell grid can lay out **every** `Scene::Text`
/// leaf, so it never defers to parley. That is the point — deferring would size
/// a terminal box with font advances, and the TUI has no fonts. `caret_bearing`
/// is likewise irrelevant here (a terminal caret is a cell position, not a
/// shaped x-offset), so an editable leaf measures on the same path as any other.
///
/// The returned box is the cell box scaled back to logical px, so taffy's
/// arithmetic stays in one unit and a **text leaf's own measured box** is a
/// whole number of cells by construction.
///
/// That guarantee does NOT extend to the whole tree: `compute_layout` resolves
/// `Percent` / `flex_grow` / `gap` in f32 and `apply` truncates to `u32`, so a
/// container split can land off the grid (four `flex_grow: 1` panes across a
/// 30-cell terminal are 7.5 cells each → 7 after the floor, and two columns go
/// unused). The paint walker floors again, so nothing overlaps or panics — the
/// cost is dropped columns at a fractional split, not corruption.
///
/// **Deferred** (R1344): remainder-distributing cell snapping — give the
/// leftover columns to the first N children so the panes tile the grid exactly,
/// which is what a cell-native solver (ratatui's own `Layout`) does. It needs a
/// snapping pass between taffy and `apply`, and no in-tree binding splits a TUI
/// pane by ratio yet, so it waits for the forcing consumer rather than
/// speculating on the rule.
impl TextMeasure for CellTextLayout {
    fn measure_text(
        &self,
        content: &str,
        style: &TextStyle,
        _runs: &[StyleRun],
        max_width: Option<u32>,
        _caret_bearing: bool,
    ) -> Option<TextBox> {
        // `None` = taffy's unbounded intrinsic-size probe. Returning the
        // natural (unwrapped) width is right for MAX-content and WRONG for
        // MIN-content, which asks for the longest unbreakable segment — the
        // `TextMeasure` seam cannot tell the two apart (its rustdoc: "`None`
        // for an unbounded min-/max-content probe"), so both get max-content.
        //
        // Consequence, and it is real: taffy's automatic minimum size then pins
        // a text leaf in a ROW flex to its whole natural width, so it overflows
        // its pane instead of wrapping. Inherited, not introduced — the parley
        // arm has the same conflation (see `layout.rs`'s available_space match)
        // and both backends therefore overflow identically, so it is not a §2 #6
        // divergence. Fixing it means widening `TextMeasure` to receive taffy's
        // `AvailableSpace` (MinContent vs MaxContent) rather than a collapsed
        // `Option<u32>`; that is a seam change touching both backends and wants
        // its own round with a real row-flex consumer to verify against.
        let max_cols = match max_width {
            Some(px) => px / self.metric.cell_w(),
            None => u32::MAX,
        };
        // R1551 — the style reaches the measure, so a paragraph's own indent is
        // budgeted for. Without it a first-line indent would break lines the
        // paint places at columns the box was never sized for.
        let (cols, rows) = self.measure_cells(content, max_cols, style);
        #[allow(
            clippy::cast_precision_loss,
            reason = "cell counts × 8/16 px stay far below f32's exact-integer range"
        )]
        let measured = TextBox {
            width: (cols * self.metric.cell_w()) as f32,
            height: (rows * self.metric.cell_h()) as f32,
            // R1344 §5.12 §2 #7 — the REAL row count. This measure wraps, so it
            // cannot borrow the single-line premise the R1070 engine holds; a
            // hardcoded 1 would make a 5-row node report "1 line" to every
            // `scene/layout` client.
            line_count: rows,
            // R1641 §5.21 — no baseline, and that is a statement rather than a
            // gap. Every cell in this grid is the same height, so every first
            // row already sits at the same offset from its box's top; the
            // top-alignment an absent baseline falls back to is the SAME
            // placement `AlignItems::Baseline` would produce. Reporting a
            // number here would claim a font metric this measure never took.
            baseline: None,
            // R1641.4 — a cell grid counts a trailing space as a cell, so
            // there is no excluded advance to report and the two widths are
            // the same number. Stated rather than left to a default, because
            // "the same" is a measurement here, not an absence.
            advance: (cols * self.metric.cell_w()) as f32,
        };
        Some(measured)
    }
}

#[cfg(test)]
mod tests {
    use super::{CellTextLayout, cell_width};
    use pinion_core::style::{TextAlign, TextIndent, TextStyle};
    use pinion_runtime::layout::TextMeasure;

    const GA: &str = "\u{AC00}"; // 가 — wide (2 cells)

    // ---- R1447 the TUI reads no fonts ----

    /// A text-heavy scene: a wrapping paragraph, a CJK run, and a styled
    /// run. Every leaf is a `Scene::Text` that a parley measure would
    /// shape, which is what makes the assertions below non-vacuous.
    fn text_heavy_scene() -> pinion_core::scene::Scene {
        use pinion_core::scene::{ContainerNode, Scene, StyleRun, TextNode};
        use pinion_core::style::{Color, SizeValue};
        let mut root = ContainerNode::default();
        root.layout.size.width = SizeValue::Percent(100);
        root.layout.size.height = SizeValue::Percent(100);
        for content in [
            "a paragraph long enough that the layout pass has to break it \
             across several lines before it fits the window",
            "\u{BB3C}\u{B54C}\u{AC00} \u{C170}\u{D55C}\u{B2E4}",
            "styled",
        ] {
            let mut text = TextNode::default();
            text.content = content.to_owned();
            text.layout.size.width = SizeValue::Percent(100);
            if content == "styled" {
                text.runs.push(StyleRun::new(
                    0,
                    3,
                    TextStyle::new().with_fg(Color::rgb(255, 0, 0)),
                ));
            }
            root.children.push(Scene::Text(text));
        }
        Scene::Container(root)
    }

    /// R1447 §5.36 §5.41 — a full TUI layout pass over real text enumerates
    /// no system fonts. §2 #6 states the TUI renders the same scene through
    /// a cell grid; this is the runtime half of that claim, since before
    /// R1447 the `LayoutCache` the pass borrows scanned every installed font
    /// on construction whether or not anything shaped.
    ///
    /// The second half is the discriminator: it shows the pass *did* measure
    /// real text, so "no font context" is a statement about the TUI and not
    /// about an empty scene. The paragraph is longer than the 40-cell window,
    /// so a pass that measured it resolves it to several rows; every leaf
    /// ends up with a non-empty box.
    ///
    /// Deliberately no assertion here that the parley arm *does* build a
    /// context — that would need a font on the host, and this crate must
    /// stay runnable with none installed (which is half of what R1447 buys).
    /// The arm comparison lives in `pinion_runtime::layout`, where fonts are
    /// a legitimate premise.
    #[test]
    fn r1447_terminal_layout_pass_builds_no_font_context() {
        use pinion_core::scene::Scene;
        use pinion_runtime::LayoutCache;

        let mut cache = LayoutCache::new();
        let mut scene = text_heavy_scene();
        let _ = super::layout_for_terminal(&mut scene, 40, 12, &mut cache);
        assert_eq!(
            cache.font_scans(),
            0,
            "the cell grid lays out every text leaf, so the TUI path never \
             reaches parley and never enumerates a font",
        );

        let Scene::Container(root) = &scene else {
            panic!("fixture root is a container");
        };
        let boxes: Vec<(u32, u32)> = root
            .children
            .iter()
            .map(|child| match child {
                Scene::Text(t) => (t.rect.w, t.rect.h),
                other => panic!("fixture children are text leaves, got {other:?}"),
            })
            .collect();
        assert!(
            boxes.iter().all(|(w, h)| *w > 0 && *h > 0),
            "premise: every leaf really measured — otherwise the assertion \
             above is about a scene with nothing to shape: {boxes:?}",
        );
        let cell_h = super::CELL.cell_h();
        assert!(
            boxes[0].1 > cell_h,
            "premise: the paragraph is wider than the 40-cell window, so a \
             pass that measured it wrapped it past one row (row={cell_h}px): \
             {boxes:?}",
        );
    }

    fn layout() -> CellTextLayout {
        CellTextLayout::default()
    }

    // ---- cell_width ----

    #[test]
    fn cell_width_scores_wide_narrow_and_zero() {
        assert_eq!(cell_width("abc"), 3, "narrow ASCII = 1 each");
        assert_eq!(cell_width(GA), 2, "CJK = 2");
        assert_eq!(cell_width(&format!("a{GA}b")), 4, "mixed");
        assert_eq!(cell_width(""), 0);
    }

    #[test]
    fn cell_width_scores_control_chars_zero_against_unicode_width() {
        // The explicit policy that keeps measure and paint in lockstep:
        // unicode-width says a C0 control is ONE cell; we say zero, because no
        // cell can hold it. Guards against a width-table change silently
        // re-introducing the budget/paint mismatch.
        use unicode_width::UnicodeWidthStr;
        assert_eq!("\n".width(), 1, "premise: unicode-width scores \\n as 1");
        assert_eq!(cell_width("\n"), 0, "but a cell cannot hold it");
        assert_eq!(cell_width("\t"), 0);
        assert_eq!(
            cell_width("a\nb"),
            2,
            "only the printable chars are budgeted"
        );
    }

    // ---- wrap SSOT ----

    #[test]
    fn wrap_soft_breaks_a_paragraph_at_the_cell_budget() {
        let text = "aaa bbb ccc";
        let lines = layout().wrap(text, 4);
        assert_eq!(lines.len(), 3, "one word per 4-cell line");
        let l = layout();
        assert_eq!(l.line_text(text, lines[0]), "aaa ");
    }

    #[test]
    fn wrap_never_yields_a_line_holding_a_break_codepoint() {
        // The D1 guard at the layout layer: whatever the breaker returns, the
        // printable slice a caller paints is free of control characters.
        let l = layout();
        for text in ["a\nb", "a\r\nb", "a\u{2028}b", "line1\nline2\n", "a\n\nb"] {
            for line in l.wrap(text, 40) {
                let printable = l.line_text(text, line);
                assert!(
                    !printable.chars().any(char::is_control),
                    "line {printable:?} of {text:?} still holds a control char",
                );
            }
        }
    }

    #[test]
    fn wrap_splits_on_hard_breaks_even_when_the_text_fits() {
        let text = "ab\ncd";
        let lines = layout().wrap(text, 80);
        assert_eq!(lines.len(), 2, "hard break splits despite a huge budget");
        let l = layout();
        assert_eq!(l.line_text(text, lines[0]), "ab");
        assert_eq!(l.line_text(text, lines[1]), "cd");
    }

    #[test]
    fn wrap_at_zero_budget_keeps_hard_structure() {
        // Documented contract: a zero budget still reflects hard breaks rather
        // than collapsing to nothing.
        let lines = layout().wrap("ab\ncd", 0);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn wrap_counts_wide_graphemes_as_two_cells() {
        // 4 wide graphemes = 8 cells. A 4-cell budget holds exactly 2 per line.
        let text = format!("{GA}{GA}{GA}{GA}");
        let lines = layout().wrap(&text, 4);
        assert!(lines.len() >= 2, "wide graphemes must wrap at 2 cells each");
        let l = layout();
        for line in &lines {
            assert!(
                cell_width(l.line_text(&text, *line)) <= 4,
                "no line exceeds the 4-cell budget",
            );
        }
    }

    // ---- R1551 paragraph placement: text-indent + text-align on cells ----

    /// The painted text of each placed line, with its start column — the pair
    /// the paint walker actually consumes.
    fn placed(text: &str, cols: u32, style: &TextStyle) -> Vec<(i32, String)> {
        let l = layout();
        l.place(text, cols, style)
            .into_iter()
            .map(|line| (line.start_col(), l.line_text(text, line.range).to_string()))
            .collect()
    }

    /// R1551 — an indent narrows the FIRST line's break budget, not the whole
    /// paragraph's. The counterfactual is in the test: the same text at the
    /// same width with no indent breaks differently.
    #[test]
    fn first_line_indent_narrows_only_the_first_line() {
        // 8px cells: a 16px indent is 2 cells.
        let style = TextStyle::new().with_text_indent(TextIndent::first_line(16));
        assert_eq!(
            placed("aaa bbb ccc", 9, &style),
            vec![(2, "aaa ".to_string()), (0, "bbb ccc".to_string())],
        );
        // Without the indent the first line holds one more word.
        assert_eq!(
            placed("aaa bbb ccc", 9, &TextStyle::new()),
            vec![(0, "aaa bbb ".to_string()), (0, "ccc".to_string())],
        );
    }

    /// R1551 — CSS `hanging` inverts the selection: the first line keeps the
    /// full width and the continuations move in.
    #[test]
    fn hanging_indent_moves_the_continuations() {
        let style = TextStyle::new().with_text_indent(TextIndent::hanging(16));
        assert_eq!(
            placed("aaa bbb ccc", 9, &style),
            vec![(0, "aaa bbb ".to_string()), (2, "ccc".to_string())],
        );
    }

    /// R1551 — `each_line` re-applies after a HARD break; without it only the
    /// paragraph's own first line is indented. Same text, same width, one flag.
    #[test]
    fn each_line_indents_after_a_hard_break() {
        let plain = TextStyle::new().with_text_indent(TextIndent::first_line(16));
        let each = TextStyle::new().with_text_indent(TextIndent::first_line(16).with_each_line());
        assert_eq!(
            placed("aa\nbb", 9, &plain),
            vec![(2, "aa".to_string()), (0, "bb".to_string())],
        );
        assert_eq!(
            placed("aa\nbb", 9, &each),
            vec![(2, "aa".to_string()), (2, "bb".to_string())],
        );
    }

    /// R1551 — a negative indent protrudes past the box's start edge, matching
    /// what parley does on the pixel path.
    #[test]
    fn negative_indent_outdents_the_first_line() {
        let style = TextStyle::new().with_text_indent(TextIndent::first_line(-16));
        let lines = placed("aa bb", 9, &style);
        assert_eq!(lines[0].0, -2, "the first line starts left of the box");
    }

    /// R1551 — the indent counts toward the measured box (an indented line
    /// occupies those cells); the ALIGNMENT does not, or an end-aligned
    /// paragraph's intrinsic width would grow with the box measuring it.
    #[test]
    fn measure_counts_the_indent_and_not_the_alignment() {
        let l = layout();
        let plain = TextStyle::new();
        let indented = TextStyle::new().with_text_indent(TextIndent::first_line(16));
        let ended = TextStyle::new().with_align(TextAlign::End);
        assert_eq!(l.measure_cells("abc", 40, &plain), (3, 1));
        assert_eq!(l.measure_cells("abc", 40, &indented), (5, 1));
        assert_eq!(
            l.measure_cells("abc", 40, &ended),
            (3, 1),
            "alignment distributes space the box already has",
        );
    }

    /// R1551 — centre and end alignment place each line inside its own box.
    /// Trailing whitespace hangs (parley subtracts it from `free_space`), so a
    /// wrapped line's trailing space does not push it off the edge.
    #[test]
    fn center_and_end_alignment_place_each_line() {
        let center = TextStyle::new().with_align(TextAlign::Center);
        let end = TextStyle::new().with_align(TextAlign::End);
        assert_eq!(
            placed("aaa bbb", 9, &center),
            vec![(1, "aaa bbb".to_string())],
        );
        assert_eq!(placed("aaa bbb", 9, &end), vec![(2, "aaa bbb".to_string())]);
        // Wrapped: line 0 is "aaa " (4 cells raw, 3 trimmed) in a 5-cell box.
        assert_eq!(
            placed("aaa bbb", 5, &end),
            vec![(2, "aaa ".to_string()), (2, "bbb".to_string())],
        );
    }

    /// R1551 — justify pads the inter-word gaps of every line EXCEPT the one
    /// that ends the paragraph, which CSS start-aligns.
    #[test]
    fn justify_pads_gaps_but_not_the_last_line() {
        let style = TextStyle::new().with_align(TextAlign::Justify);
        let l = layout();
        let lines = l.place("aa bb cc dd", 8, &style);
        assert_eq!(lines.len(), 2, "breaks into two lines at 8 cells");
        // Line 0 is "aa bb " — 5 printable cells trimmed, 3 free, 1 gap.
        assert_eq!(lines[0].gap_pad.each, 3);
        assert_eq!(lines[0].gap_pad.leading_extra, 0);
        assert_eq!(
            lines[0].align_col, 0,
            "justify pads gaps, it does not shift"
        );
        // The paragraph's last line is never justified.
        assert!(lines[1].gap_pad.is_none());
    }

    /// R1551 — the remainder of an uneven division goes to the LEADING gaps, so
    /// the padded width is exactly the free space and the right edge lands on
    /// the box edge.
    #[test]
    fn justify_remainder_lands_on_the_leading_gaps() {
        let style = TextStyle::new().with_align(TextAlign::Justify);
        let l = layout();
        // "a b c " is 5 printable cells with 2 gaps in a 10-cell box: 5 free
        // cells over 2 gaps does not divide.
        let src = "a b c dddddddddd";
        let lines = l.place(src, 10, &style);
        let first = lines[0];
        let pad = first.gap_pad;
        let total: u32 = (0..2).map(|i| pad.for_gap(i)).sum();
        let text = l.line_text(src, first.range);
        let width = u32::try_from(cell_width(text.trim_end())).unwrap();
        assert_eq!(width + total, 10, "a justified line fills its box exactly");
        assert_eq!(pad.for_gap(0), pad.for_gap(1) + 1, "remainder goes first");
    }

    /// R1551 — a line that ends at a HARD break is not justified either: it
    /// ends its paragraph's scope even though more lines follow.
    #[test]
    fn justify_skips_a_line_ending_at_a_hard_break() {
        let style = TextStyle::new().with_align(TextAlign::Justify);
        let lines = layout().place("a b\nc d e f", 9, &style);
        assert!(
            lines[0].gap_pad.is_none(),
            "the line before the newline ends its scope",
        );
    }

    /// R1551 — `wrap` is `place` at the CSS initial values, so the two cannot
    /// disagree about where a plain paragraph breaks.
    #[test]
    fn wrap_is_place_at_the_initial_style() {
        let l = layout();
        for text in ["aa bb cc", "가나 다라", "aaaaaaaa\nbb", ""] {
            for cols in [0_u32, 1, 3, 5, 40] {
                let ranges: Vec<_> = l
                    .place(text, cols, &TextStyle::new())
                    .into_iter()
                    .map(|line| line.range)
                    .collect();
                assert_eq!(l.wrap(text, cols), ranges, "{text:?} at {cols}");
            }
        }
    }

    // ---- px conversion + measure ----

    #[test]
    fn wrap_px_floors_the_budget_to_whole_cells() {
        // 8px per cell: a 20px box is 2.5 cells → 2.
        let text = "aa bb";
        assert_eq!(layout().wrap_px(text, 20), layout().wrap(text, 2));
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "cell counts × 8/16 px are exact integers in f32 — the box is                   either on the cell grid or the measure is wrong"
    )]
    fn measure_returns_a_cell_aligned_box() {
        let l = layout();
        let m = l
            .measure_text("ab\ncd", &TextStyle::default(), &[], Some(80), false)
            .expect("cell layout always measures");
        assert_eq!(m.width, 16.0, "2 cells wide × 8px");
        assert_eq!(m.height, 32.0, "2 rows × 16px");
        assert_eq!(m.line_count, 2, "and it REPORTS its two lines");
    }

    #[test]
    fn measure_never_defers_to_parley() {
        // The contract that keeps fonts out of the TUI: every leaf measures here,
        // including a caret-bearing (editable) one.
        let l = layout();
        for caret in [false, true] {
            assert!(
                l.measure_text("x", &TextStyle::default(), &[], Some(80), caret)
                    .is_some(),
                "caret_bearing={caret} must still measure on the cell grid",
            );
        }
        assert!(
            l.measure_text("", &TextStyle::default(), &[], None, false)
                .is_some(),
            "unbounded intrinsic probe measures too",
        );
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        clippy::cast_precision_loss,
        reason = "row count × 16 px is an exact integer in f32 — measure and wrap                   either agree exactly or the box is sized for the wrong row count"
    )]
    fn measure_agrees_with_wrap_row_count() {
        // The measure/paint coherence invariant, at the layer where it is
        // decided: the box height a node gets is exactly the number of rows the
        // paint walker will emit.
        let l = layout();
        let text = "alpha beta gamma delta epsilon";
        let max_px = 80; // 10 cells
        let rows = l.wrap_px(text, max_px).len();
        let m = l
            .measure_text(text, &TextStyle::default(), &[], Some(max_px), false)
            .expect("measures");
        assert_eq!(
            m.height,
            (rows * 16) as f32,
            "measured height must equal wrapped rows × cell height",
        );
        assert_eq!(
            m.line_count,
            u32::try_from(rows).unwrap(),
            "and the reported line_count must equal the wrapped rows — this is \
             the §5.12 datum an AI client reads instead of pixels",
        );
    }
}
