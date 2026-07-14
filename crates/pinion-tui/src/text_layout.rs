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
//! ([`pinion_text_unicode::wrap_paragraph_with_measure`], §5.37.7) and supplies
//! the only genuinely backend-specific input: how wide a segment is, in cells.
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
//! ## One SSOT, two callers
//!
//! [`CellTextLayout::wrap`] is called from both:
//!
//! * the **measure** pass — `impl TextMeasure`, consulted by
//!   `compute_layout_with_text_measure` to size a `Scene::Text` node; and
//! * the **paint** pass — [`crate::paint`], to place each row.
//!
//! They must agree exactly, or a box sized for N rows gets N±1 rows of text. The
//! R1070 Vello precedent learned this the hard way (see `TextMeasure`'s rustdoc
//! on sharing the `single_line_overflows` SSOT between its measure and paint
//! arms); here the coupling is structural — there is one `wrap` and both call it.
//!
//! ## Control characters never reach a cell
//!
//! [`pinion_text_unicode::wrap_paragraph_with_measure`] emits line ranges that
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
use pinion_core::style::TextStyle;
use pinion_runtime::layout::{TextBox, TextMeasure};
use pinion_runtime::{LayoutCache, compute_layout_with_text_measure};
use pinion_text_unicode::{LineRange, trim_trailing_break, wrap_paragraph_with_measure};
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
    #[must_use]
    pub fn wrap(self, content: &str, max_cols: u32) -> Vec<LineRange> {
        #[allow(
            clippy::cast_precision_loss,
            reason = "max_cols is a terminal column count — far below f32's exact-integer range"
        )]
        let max_width = max_cols as f32;
        wrap_paragraph_with_measure(content, max_width, |seg| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a segment's cell width is bounded by the text length"
            )]
            let w = cell_width(seg) as f32;
            w
        })
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

    /// The laid-out box for `content` in cells: `(cols, rows)`.
    ///
    /// `cols` is the widest printable line (never more than `max_cols` unless a
    /// single unbreakable segment overflows — the breaker's documented overflow
    /// rule, which the paint walker clips). `rows` is the line count.
    #[must_use]
    pub fn measure_cells(self, content: &str, max_cols: u32) -> (u32, u32) {
        let lines = self.wrap(content, max_cols);
        let cols = lines
            .iter()
            .map(|l| cell_width(self.line_text(content, *l)))
            .max()
            .unwrap_or(0);
        (
            u32::try_from(cols).unwrap_or(u32::MAX),
            u32::try_from(lines.len()).unwrap_or(u32::MAX),
        )
    }
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
) -> bool {
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
) -> bool {
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
        _style: &TextStyle,
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
        let (cols, rows) = self.measure_cells(content, max_cols);
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
        };
        Some(measured)
    }
}

#[cfg(test)]
mod tests {
    use super::{CellTextLayout, cell_width};
    use pinion_core::style::TextStyle;
    use pinion_runtime::layout::TextMeasure;

    const GA: &str = "\u{AC00}"; // 가 — wide (2 cells)

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
