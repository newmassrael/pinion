//! R50.12 §5.37.6 — multi-line shaped paragraph layout (wrap + shape + stack).
//!
//! The last pure-CPU layer of the self-hosted text engine before paint wiring:
//! it joins the two halves built up to here — the UAX #14 greedy line breaker
//! ([`crate::wrap_paragraph`], §5.37.7) and the UAX #9 paragraph shaper
//! ([`crate::shape_paragraph`], §5.37.6) — into a wrapped, shaped block of
//! lines, each carrying its glyphs and a baseline. [`layout_paragraph_with_fallback`]
//! is the multi-font analogue: it wraps and shapes across a font stack with
//! per-codepoint fallback (§5.37.10), so a mixed-script paragraph breaks at its
//! true advances and each glyph carries its fallback font's stack index.
//!
//! BIDI levels are **paragraph-context**, resolved once for the whole paragraph
//! ([`pinion_text_unicode::itemize()`]); a line is then a *slice* of that
//! resolution, never a re-resolution of the line's substring. Re-resolving a
//! line alone would mis-derive its base direction (UAX #9 P2/P3 look at the
//! paragraph's first strong character, not the line's) and its embedding
//! levels. The shaper instead clips the paragraph's runs to each line's byte
//! range and applies UAX #9 L2 reordering per line — which is exactly where L2
//! belongs ("on each line"). Width is summed advance and so reorder-invariant,
//! so the break points from `wrap_paragraph` are unaffected by direction.
//!
//! Vertical metrics come from `hhea`: each line box is
//! `ascender - descender + line_gap` tall (device px), baselines step by that
//! height, and the first baseline sits at `ascender`.
//!
//! Scope (honest deferrals, not silent gaps): expects a **single BIDI
//! paragraph** (like [`crate::shape_paragraph`] / [`pinion_text_unicode::itemize()`]).
//! The caller splits hard paragraph separators (`Bidi_Class = B`: `\n`, U+2029)
//! first via [`pinion_text_unicode::bidi::iter_paragraphs`]; the line separators
//! handled internally are soft wraps and U+2028 (`WS`, a UAX #14 mandatory
//! break that stays within one BIDI paragraph). UAX #9 L1's *per-line* trailing
//! whitespace reset is not yet applied (levels carry the paragraph-as-one-line
//! L1 from [`pinion_text_unicode::resolved_levels`]); a line range includes its
//! trailing break codepoint (from `wrap_paragraph`), which is shaped rather than
//! trimmed. Production paint is still §5.36 swash, so [`layout_paragraph`] is a
//! test forcing-consumer until the paint path wires the self-hosted layout.

use crate::Font;
use crate::paragraph::{PlacementRun, font_split_runs, shape_runs_visual, single_font_runs};
use crate::raster::{Coverage, RasterError};
use crate::shape::{GlyphDraw, PositionedGlyph, render_glyphs};
use crate::wrap::{LineRange, wrap_paragraph, wrap_paragraph_with_fallback};
use pinion_text_unicode::{ItemRun, itemize};

/// One laid-out, shaped line of a paragraph.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedLine {
    /// Glyphs in visual (left-to-right) order, `x` ascending. `x` is the
    /// pen-origin x in device px **relative to this line's left edge**;
    /// `cluster` is the paragraph byte offset of the source codepoint (so it
    /// descends across a right-to-left run). A renderer draws each glyph at
    /// `(x, baseline)`.
    pub glyphs: Vec<PositionedGlyph>,
    /// Total advance width of the line (device px).
    pub advance: f32,
    /// The line's logical byte range in the paragraph (from
    /// [`wrap_paragraph`]); contiguous with its neighbours.
    pub range: LineRange,
    /// Baseline y of this line (device px, measured down from the block top).
    pub baseline: f32,
}

/// A wrapped, shaped paragraph: lines stacked top to bottom.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedLines {
    /// The lines in top-to-bottom order; empty for an empty paragraph.
    pub lines: Vec<ShapedLine>,
    /// Width actually used: the widest line's advance (device px).
    pub width: f32,
    /// Total block height: `lines.len()` line boxes, each
    /// `ascender - descender + line_gap` tall (device px).
    pub height: f32,
}

/// Wrap `paragraph` to `max_width` (device px at `px_per_em`) and shape each
/// line into visually-ordered positioned glyphs (§5.37.6 + §5.37.7): break with
/// the UAX #14 greedy breaker, resolve BIDI once for the whole paragraph, then
/// clip the paragraph's runs to each line and apply UAX #9 L2 per line. Expects
/// a single BIDI paragraph; an empty string yields no lines.
#[must_use]
pub fn layout_paragraph(
    font: &Font,
    paragraph: &str,
    px_per_em: f32,
    max_width: f32,
) -> ShapedLines {
    let line_ranges = wrap_paragraph(font, paragraph, px_per_em, max_width);
    if line_ranges.is_empty() {
        return ShapedLines::default();
    }
    // Resolve BIDI + script once for the whole paragraph; lines are slices of
    // this single resolution (paragraph-context levels), never re-resolved.
    let para_runs = itemize(paragraph);
    // Single-font multi-line: every clipped run shapes in `font` (index 0).
    assemble_lines(
        &[font],
        paragraph,
        px_per_em,
        &line_ranges,
        &para_runs,
        single_font_runs,
    )
}

/// Wrap `paragraph` to `max_width` and shape each line across the `fonts` stack
/// with per-codepoint fallback (§5.37.6 + §5.37.7 + §5.37.10) — the multi-font
/// analogue of [`layout_paragraph`], joining the fallback shaper
/// ([`crate::shape_paragraph_with_fallback`]) with the line breaker. Wrapping
/// measures advances across the stack ([`wrap_paragraph_with_fallback`]) so a
/// mixed-script paragraph breaks at its true widths, and each wrapped line's
/// clipped runs are font-split ([`font_split_runs`]) before shaping, so every
/// glyph carries the stack index of the font that shaped it (a renderer
/// rasterizes it with `fonts[g.font_index]`; [`render_lines`] does exactly that).
/// Vertical metrics come from the primary font (`fonts[0]`); a one-element stack
/// reproduces [`layout_paragraph`] exactly. Expects a single BIDI paragraph; an
/// empty string or empty stack yields no lines.
#[must_use]
pub fn layout_paragraph_with_fallback(
    fonts: &[&Font],
    paragraph: &str,
    px_per_em: f32,
    max_width: f32,
) -> ShapedLines {
    if fonts.is_empty() {
        return ShapedLines::default();
    }
    let line_ranges = wrap_paragraph_with_fallback(fonts, paragraph, px_per_em, max_width);
    if line_ranges.is_empty() {
        return ShapedLines::default();
    }
    let para_runs = itemize(paragraph);
    assemble_lines(
        fonts,
        paragraph,
        px_per_em,
        &line_ranges,
        &para_runs,
        |clipped| font_split_runs(fonts, paragraph, clipped),
    )
}

/// Shape and stack the wrapped `line_ranges` into a [`ShapedLines`] block — the
/// core shared by [`layout_paragraph`] (single-font feeder) and
/// [`layout_paragraph_with_fallback`] (font-split feeder). Only the per-line
/// run-builder differs, so the vertical-metric computation and per-line stacking
/// live here once. `para_runs` are the paragraph's itemised runs (resolved once,
/// never per line); `build_runs` maps each line's clipped runs onto placement
/// runs over `fonts`. Vertical metrics come from `fonts[0]` (the caller
/// guarantees a non-empty stack and non-empty `line_ranges`).
fn assemble_lines(
    fonts: &[&Font],
    paragraph: &str,
    px_per_em: f32,
    line_ranges: &[LineRange],
    para_runs: &[ItemRun],
    build_runs: impl Fn(&[ItemRun]) -> Vec<PlacementRun>,
) -> ShapedLines {
    // Vertical metrics from the primary font's hhea, guarded like `shape_run` (a
    // degenerate units-per-em or a non-finite / non-positive size collapses to
    // zero). Mixing per-line metrics across fallback fonts is a deeper typographic
    // concern (line box = max run ascent); the primary font sets the line box.
    let font = fonts[0];
    let upem = font.units_per_em();
    let scale = if upem == 0 || !px_per_em.is_finite() || px_per_em <= 0.0 {
        0.0
    } else {
        px_per_em / f32::from(upem)
    };
    let ascent = f32::from(font.ascender()) * scale;
    let descent = f32::from(font.descender()) * scale; // <= 0
    let line_gap = f32::from(font.line_gap()) * scale;
    // `line_height` adds a subtraction `shape_run`'s pure `+=` never performs, so a
    // malformed font (metrics > upem) under an extreme finite `px_per_em` can scale
    // to opposite-sign infinities and make this NaN. Collapse any non-finite result
    // to a zero (degenerate) vertical layout rather than poison every baseline.
    let (ascent, line_height) = {
        let lh = ascent - descent + line_gap;
        if lh.is_finite() {
            (ascent, lh)
        } else {
            (0.0, 0.0)
        }
    };

    let mut lines: Vec<ShapedLine> = Vec::with_capacity(line_ranges.len());
    let mut width = 0.0_f32;
    let mut height = 0.0_f32; // running top offset, also the final block height
    for range in line_ranges {
        let clipped = clip_runs(para_runs, range.start, range.end);
        let placed = build_runs(&clipped);
        let shaped = shape_runs_visual(fonts, paragraph, px_per_em, &placed);
        width = width.max(shaped.advance);
        lines.push(ShapedLine {
            glyphs: shaped.glyphs,
            advance: shaped.advance,
            range: *range,
            baseline: ascent + height,
        });
        height += line_height;
    }

    ShapedLines {
        lines,
        width,
        height,
    }
}

/// Rasterize a wrapped, shaped paragraph ([`layout_paragraph`]) into one
/// anti-aliased coverage bitmap, stacking every line at its baseline — the
/// multi-line analogue of [`crate::paragraph::render_paragraph`]. Each line's
/// glyphs are placed at `(glyph.x, line.baseline + glyph.y)` (line-relative pen x,
/// block-top-relative baseline) and composited through one glyph atlas per stack
/// font. `fonts` must be the stack the lines were shaped with: single-font
/// [`layout_paragraph`] tags every glyph `font_index` 0, so a one-font stack
/// suffices, while [`layout_paragraph_with_fallback`] tags each glyph with its
/// fallback font's stack index — this rasterizes each from that font. Returns
/// [`Coverage::empty`] for no lines or an empty stack. Production paint is still
/// §5.36 swash, so this is a test forcing-consumer.
///
/// # Errors
///
/// Propagates a [`RasterError`] from any glyph's rasterization (a pathological
/// `px_per_em` or a not-yet-supported composite glyph).
pub fn render_lines(
    fonts: &[&Font],
    lines: &ShapedLines,
    px_per_em: f32,
) -> Result<Coverage, RasterError> {
    render_glyphs(
        fonts,
        px_per_em,
        lines.lines.iter().flat_map(|line| {
            let baseline = line.baseline;
            line.glyphs.iter().map(move |g| GlyphDraw {
                font_index: g.font_index,
                glyph_id: g.glyph_id,
                pen_x: g.x,
                pen_y: baseline + g.y,
            })
        }),
    )
}

/// Clip the paragraph's itemised runs to the byte range `start..end`, keeping
/// each run's resolved level and script. A line boundary always lands on a
/// UAX #14 opportunity (a codepoint boundary), so a straddling run is split
/// cleanly; runs fully outside the range are dropped. The clipped sub-runs stay
/// uniform in level and script — the property the shaper relies on.
fn clip_runs(runs: &[ItemRun], start: usize, end: usize) -> Vec<ItemRun> {
    runs.iter()
        .filter_map(|r| {
            let s = r.start.max(start);
            let e = r.end.min(end);
            (s < e).then_some(ItemRun {
                level: r.level,
                script: r.script,
                start: s,
                end: e,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ShapedLines, layout_paragraph, layout_paragraph_with_fallback, render_lines};
    use crate::fallback::shape_with_fallback;
    use crate::raster::Coverage;
    use crate::shape::shape_run;
    use crate::{Font, render_paragraph, shape_paragraph, shape_paragraph_with_fallback};
    use pinion_text_unicode::{is_removed_by_x9, reorder_visual, resolved_levels};

    const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
    const NANUM: &str = "tests/fonts/NanumGothic-Regular.ttf";
    /// Hangul GA / NA / DA (U+AC00 U+B098 U+B2E4) — uncovered by `NotoSans`,
    /// covered by `NanumGothic`; written escaped to keep the source ASCII-only.
    const HANGUL: &str = "\u{AC00}\u{B098}\u{B2E4}";
    const PX: f32 = 32.0;

    fn font() -> Font {
        load(NOTO)
    }

    fn load(path: &str) -> Font {
        let bytes = std::fs::read(path).expect("read font fixture");
        Font::from_bytes(bytes).expect("valid Font")
    }

    /// `(ascent, line_height)` in device px at [`PX`], from the same hhea metrics
    /// `layout_paragraph` uses — the test oracle for vertical placement.
    fn vmetrics(f: &Font) -> (f32, f32) {
        let scale = PX / f32::from(f.units_per_em());
        let ascent = f32::from(f.ascender()) * scale;
        let descent = f32::from(f.descender()) * scale;
        let gap = f32::from(f.line_gap()) * scale;
        (ascent, ascent - descent + gap)
    }

    /// The visual cluster order a line *should* have: paragraph-context BIDI
    /// levels (resolved over the whole paragraph), restricted to the line's
    /// visible codepoints, reordered by UAX #9 L2 at codepoint granularity.
    /// Independent of `layout_paragraph`'s run-granularity path, so a match
    /// proves the two agree. Valid only for inputs with one glyph per codepoint
    /// (no ligatures / combining marks).
    fn expected_visual_clusters(paragraph: &str, start: usize, end: usize) -> Vec<usize> {
        let levels = resolved_levels(paragraph);
        let chars: Vec<char> = paragraph.chars().collect();
        let byte_of: Vec<usize> = paragraph.char_indices().map(|(b, _)| b).collect();
        let line: Vec<usize> = (0..chars.len())
            .filter(|&i| byte_of[i] >= start && byte_of[i] < end)
            .filter(|&i| !is_removed_by_x9(chars[i]))
            .collect();
        let line_levels: Vec<u8> = line.iter().map(|&i| levels[i]).collect();
        reorder_visual(&line_levels)
            .into_iter()
            .map(|k| byte_of[line[k]])
            .collect()
    }

    fn clusters(line: &super::ShapedLine) -> Vec<usize> {
        line.glyphs.iter().map(|g| g.cluster).collect()
    }

    #[test]
    fn empty_paragraph_has_no_lines() {
        let got = layout_paragraph(&font(), "", PX, 1000.0);
        // Struct equality proves width/height are the 0.0 default and lines empty.
        assert_eq!(got, ShapedLines::default());
        assert!(got.lines.is_empty());
    }

    #[test]
    fn single_line_equals_shape_paragraph() {
        // Unbounded width, single-script LTR text: exactly one line whose glyphs
        // and advance equal a plain `shape_paragraph`, baseline at the ascent.
        let f = font();
        let text = "the quick brown fox";
        let laid = layout_paragraph(&f, text, PX, f32::MAX);
        let para = shape_paragraph(&f, text, PX);
        assert_eq!(laid.lines.len(), 1);
        let line = &laid.lines[0];
        assert_eq!(line.glyphs, para.glyphs, "single line == shape_paragraph");
        assert!((line.advance - para.advance).abs() < 1e-4);
        assert_eq!(line.range.start, 0);
        assert_eq!(line.range.end, text.len());
        let (ascent, line_height) = vmetrics(&f);
        assert!(
            (line.baseline - ascent).abs() < 1e-4,
            "first baseline = ascent"
        );
        assert!((laid.width - para.advance).abs() < 1e-4);
        assert!((laid.height - line_height).abs() < 1e-4);
    }

    #[test]
    fn baselines_step_by_line_height() {
        // Three lines via U+2028 (a UAX #14 mandatory break that stays in one
        // BIDI paragraph): baselines must be ascent, ascent+h, ascent+2h, and
        // the block height 3h — pinned to the font's own hhea metrics.
        let f = font();
        let laid = layout_paragraph(&f, "ab\u{2028}cd\u{2028}ef", PX, f32::MAX);
        assert_eq!(laid.lines.len(), 3, "two mandatory breaks => three lines");
        let (ascent, line_height) = vmetrics(&f);
        let mut expect = ascent;
        for line in &laid.lines {
            assert!(
                (line.baseline - expect).abs() < 1e-3,
                "baseline {} != {expect}",
                line.baseline
            );
            expect += line_height;
        }
        assert!((laid.height - line_height * 3.0).abs() < 1e-3);
    }

    #[test]
    fn lines_tile_and_cover_every_visible_cluster() {
        // Wrapped LTR text: line ranges tile [0, len) contiguously and every
        // codepoint's byte offset surfaces as exactly one glyph cluster.
        let f = font();
        let text = "alpha beta gamma delta epsilon";
        let max = shape_paragraph(&f, "alpha beta ", PX).advance;
        let laid = layout_paragraph(&f, text, PX, max);
        assert!(laid.lines.len() >= 2, "must wrap");
        assert_eq!(laid.lines[0].range.start, 0);
        assert_eq!(laid.lines.last().unwrap().range.end, text.len());
        for pair in laid.lines.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start, "contiguous");
        }
        let mut seen: Vec<usize> = laid.lines.iter().flat_map(clusters).collect();
        seen.sort_unstable();
        let want: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        assert_eq!(seen, want, "every codepoint shaped exactly once");
    }

    #[test]
    fn mandatory_break_splits_and_shapes_each_line() {
        // "ab\u{2028}cd": two lines, each independently shaped.
        let f = font();
        let laid = layout_paragraph(&f, "ab\u{2028}cd", PX, f32::MAX);
        assert_eq!(laid.lines.len(), 2);
        assert_eq!(&clusters(&laid.lines[0])[..2], &[0, 1], "line 0 = a, b");
        // line 1 = c, d at bytes 5, 6 (U+2028 is 3 bytes: 2..5).
        assert_eq!(clusters(&laid.lines[1]), vec![5, 6], "line 1 = c, d");
    }

    #[test]
    fn rtl_run_mirrors_within_its_own_line() {
        // Base LTR ("ab" first). After the mandatory break the Hebrew word is a
        // pure RTL line: its clusters must DESCEND in visual order (alef, the
        // logically-first codepoint, sits rightmost) — per-line L2 on a clipped
        // run. Bytes: a0 b1 U+2028=2..5, alef 5, bet 7, gimel 9.
        let f = font();
        let laid = layout_paragraph(&f, "ab\u{2028}\u{05D0}\u{05D1}\u{05D2}", PX, f32::MAX);
        assert_eq!(laid.lines.len(), 2);
        assert_eq!(
            clusters(&laid.lines[1]),
            vec![9, 7, 5],
            "Hebrew line mirrors within itself"
        );
    }

    #[test]
    fn wrapped_rtl_line_pins_glyph_x_geometry() {
        // Stronger than order: pin the wrapped RTL line's glyph x. The first
        // visual glyph sits at the line origin (x ~= 0) and each glyph's x equals
        // the shape_run-derived mirror (run_advance - next_logical_pen_x), so a
        // regression in the per-line x-mirror math is caught at the layout level,
        // not only through the shared-core paragraph test.
        let f = font();
        let laid = layout_paragraph(&f, "ab\u{2028}\u{05D0}\u{05D1}\u{05D2}", PX, f32::MAX);
        let line = &laid.lines[1]; // pure Hebrew RTL line, bytes 5 / 7 / 9
        assert!(
            line.glyphs[0].x.abs() < 1e-4,
            "first visual glyph at line origin"
        );
        let logical = shape_run(&f, "\u{05D0}\u{05D1}\u{05D2}", PX);
        let n = logical.glyphs.len();
        for g in &line.glyphs {
            let li = (g.cluster - 5) / 2; // paragraph byte -> Hebrew codepoint index
            let next_x = if li + 1 < n {
                logical.glyphs[li + 1].x
            } else {
                logical.advance
            };
            let expected = logical.advance - next_x;
            assert!(
                (g.x - expected).abs() < 1e-4,
                "cluster {} x {} != mirror {expected}",
                g.cluster,
                g.x
            );
        }
    }

    #[test]
    fn per_line_order_uses_paragraph_context_not_reresolution() {
        // Base LTR (leading 'a'). The text wraps at U+2028 into two mixed
        // Latin+Hebrew lines. Each line's visual order must match the oracle
        // built from PARAGRAPH-context levels; the second line, re-resolved in
        // isolation, would have an RTL base and a different order — the negative
        // control that would fail had layout re-resolved BIDI per line.
        let f = font();
        let text = "ab\u{05D0}\u{05D1}\u{2028}\u{05D2}\u{05D3}cd";
        let laid = layout_paragraph(&f, text, PX, f32::MAX);
        assert_eq!(laid.lines.len(), 2);
        for line in &laid.lines {
            assert_eq!(
                clusters(line),
                expected_visual_clusters(text, line.range.start, line.range.end),
                "line {:?} must match paragraph-context oracle",
                line.range
            );
        }

        // Negative control: the second line's paragraph-context levels differ
        // from re-resolving its substring alone (RTL base), so the oracle match
        // genuinely discriminates against per-line re-resolution.
        let l1 = laid.lines.last().unwrap();
        let para_levels = resolved_levels(text);
        let byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        let sliced: Vec<u8> = (0..byte_of.len())
            .filter(|&i| byte_of[i] >= l1.range.start && byte_of[i] < l1.range.end)
            .map(|i| para_levels[i])
            .collect();
        let alone = resolved_levels(&text[l1.range.start..l1.range.end]);
        assert_ne!(
            sliced, alone,
            "paragraph-context vs isolated levels must differ for the test to bite"
        );
    }

    // ---- render_lines (R1052 multi-line block rasterization) ----

    #[test]
    fn render_lines_empty_or_no_stack_is_empty() {
        let f = font();
        let none = layout_paragraph(&f, "", PX, 1000.0); // no lines
        assert_eq!(
            render_lines(&[&f], &none, PX).unwrap(),
            Coverage::empty(),
            "no lines => empty"
        );
        let one = layout_paragraph(&f, "A", PX, f32::MAX);
        assert_eq!(
            render_lines(&[], &one, PX).unwrap(),
            Coverage::empty(),
            "empty stack => empty"
        );
    }

    #[test]
    fn render_lines_single_line_ink_matches_render_paragraph() {
        // One unwrapped line renders the SAME ink as render_paragraph; layout places
        // the line at its baseline while render_paragraph uses origin 0, so only the
        // bounding box `top` differs (by round(baseline)) — alpha and dimensions are
        // identical. This pins render_lines onto the validated single-line renderer.
        let f = font();
        let text = "the quick brown fox";
        let lines = layout_paragraph(&f, text, PX, f32::MAX);
        assert_eq!(lines.lines.len(), 1, "unbounded width => one line");
        let block = render_lines(&[&f], &lines, PX).unwrap();
        let para = render_paragraph(&[&f], &shape_paragraph(&f, text, PX), PX).unwrap();
        assert_eq!(block.alpha, para.alpha, "identical ink");
        assert_eq!((block.width, block.height), (para.width, para.height));
        assert_eq!(block.left, para.left);
        #[allow(clippy::cast_possible_truncation)]
        let baseline = lines.lines[0].baseline.round() as i32;
        assert_eq!(
            block.top,
            para.top + baseline,
            "shifted down by the baseline"
        );
    }

    #[test]
    fn render_lines_stacks_each_line_by_line_height() {
        // Two identical lines via a mandatory break: the block grows over a single
        // line by exactly one line box (the font's hhea line height), proving lines
        // stack at stepped baselines instead of overdrawing at the same y.
        let f = font();
        let (_, line_height) = vmetrics(&f);
        let one = render_lines(&[&f], &layout_paragraph(&f, "ab", PX, f32::MAX), PX).unwrap();
        let two = render_lines(
            &[&f],
            &layout_paragraph(&f, "ab\u{2028}ab", PX, f32::MAX),
            PX,
        )
        .unwrap();
        assert!(!two.alpha.is_empty(), "two-line block has ink");
        let grew = two.height.saturating_sub(one.height); // usize, two is the taller
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = line_height.round() as usize;
        assert!(
            grew.abs_diff(expected) <= 2,
            "two-line block taller than one by ~line_height ({line_height}); grew {grew}"
        );
    }

    #[test]
    fn render_lines_places_second_line_a_line_height_below_first() {
        // Two single-glyph lines ("x" twice): the composite must show TWO separate
        // ink bands — a blank-row gap between them — the second starting
        // ~round(line_height) below the first. Pins internal per-line stacking
        // geometry, not just the total height delta (an overdraw or mis-stepped
        // baseline that preserved total extent would still be caught here).
        let f = font();
        let (_, line_height) = vmetrics(&f);
        let cov =
            render_lines(&[&f], &layout_paragraph(&f, "x\u{2028}x", PX, f32::MAX), PX).unwrap();
        let inked: Vec<usize> = (0..cov.height)
            .filter(|&r| {
                cov.alpha[r * cov.width..(r + 1) * cov.width]
                    .iter()
                    .any(|&a| a != 0)
            })
            .collect();
        assert!(inked.len() >= 2, "two lines must ink some rows");
        // The largest jump between consecutive inked rows splits the two bands.
        let gap_at = (1..inked.len())
            .max_by_key(|&i| inked[i] - inked[i - 1])
            .expect("at least two inked rows");
        assert!(
            inked[gap_at] - inked[gap_at - 1] > 1,
            "stepped baselines leave a blank-row gap, not one merged band"
        );
        let step = inked[gap_at] - inked[0]; // top of band 1 minus top of band 0
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = line_height.round() as usize;
        assert!(
            step.abs_diff(expected) <= 2,
            "second line starts ~line_height ({expected}) below the first; got {step}"
        );
    }

    // ---- layout_paragraph_with_fallback (R1054 multi-font multi-line) ----

    #[test]
    fn fallback_single_font_stack_equals_layout_paragraph() {
        // A one-element stack reproduces the single-font layout exactly (every glyph
        // font_index 0, same wrap, same baselines): layout_paragraph is the 1-stack
        // special case of layout_paragraph_with_fallback.
        let f = font();
        let narrow = shape_paragraph(&f, "alpha beta ", PX).advance;
        let cases: [(&str, f32); 2] = [
            ("the quick brown fox", f32::MAX),
            ("alpha beta gamma delta epsilon", narrow),
        ];
        for (text, max) in cases {
            assert_eq!(
                layout_paragraph_with_fallback(&[&f], text, PX, max),
                layout_paragraph(&f, text, PX, max),
                "1-font stack == single-font layout for {text:?}",
            );
        }
    }

    #[test]
    fn wrapped_mixed_script_tags_fonts_per_codepoint() {
        // A paragraph alternating Latin and Hangul, wrapped to several lines: every
        // glyph must carry the stack index of the font covering its codepoint —
        // Latin -> Noto (0), Hangul -> Nanum (1) — across the wrap, and the lines
        // must still tile [0, len) covering each codepoint once. Proves wrapping and
        // fallback compose, not just one in isolation.
        let noto = font();
        let nanum = load(NANUM);
        let stack = [&noto, &nanum];
        let text = format!("abc {HANGUL} xyz {HANGUL} def");
        let max = shape_with_fallback(&stack, &format!("abc {HANGUL} "), PX).advance;
        let laid = layout_paragraph_with_fallback(&stack, &text, PX, max);
        assert!(laid.lines.len() >= 2, "mixed paragraph must wrap");

        for line in &laid.lines {
            for g in &line.glyphs {
                let ch = text[g.cluster..]
                    .chars()
                    .next()
                    .expect("cluster lands on a char start");
                let want = usize::from(!ch.is_ascii());
                assert_eq!(
                    g.font_index, want,
                    "cluster {} ({ch:?}) font_index",
                    g.cluster,
                );
            }
        }

        // Lines tile and cover every codepoint exactly once.
        assert_eq!(laid.lines[0].range.start, 0);
        assert_eq!(laid.lines.last().unwrap().range.end, text.len());
        for pair in laid.lines.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start, "contiguous");
        }
        let mut seen: Vec<usize> = laid
            .lines
            .iter()
            .flat_map(|l| l.glyphs.iter().map(|g| g.cluster))
            .collect();
        seen.sort_unstable();
        let want: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        assert_eq!(seen, want, "every codepoint shaped exactly once");
    }

    #[test]
    fn render_lines_multi_font_ink_matches_render_paragraph() {
        // One unwrapped mixed-script line: layout_paragraph_with_fallback +
        // render_lines must produce the SAME ink as shape_paragraph_with_fallback +
        // render_paragraph (both feed the shared render_glyphs core), shifted down by
        // the baseline. render_paragraph's multi-font rasterization is separately
        // proven (R1050/R1051), so this transitively pins render_lines feeding the
        // fallback stack — the Hangul rasterizes from Nanum, the Latin from Noto.
        let noto = font();
        let nanum = load(NANUM);
        let stack = [&noto, &nanum];
        let text = format!("ab {HANGUL} cd");
        let laid = layout_paragraph_with_fallback(&stack, &text, PX, f32::MAX);
        assert_eq!(laid.lines.len(), 1, "unbounded width => one line");
        let block = render_lines(&stack, &laid, PX).unwrap();
        let para = render_paragraph(
            &stack,
            &shape_paragraph_with_fallback(&stack, &text, PX),
            PX,
        )
        .unwrap();
        assert!(!block.alpha.is_empty(), "mixed line has ink");
        assert_eq!(block.alpha, para.alpha, "identical multi-font ink");
        assert_eq!((block.width, block.height), (para.width, para.height));
        assert_eq!(block.left, para.left);
        #[allow(clippy::cast_possible_truncation)]
        let baseline = laid.lines[0].baseline.round() as i32;
        assert_eq!(
            block.top,
            para.top + baseline,
            "shifted down by the baseline"
        );
    }
}
