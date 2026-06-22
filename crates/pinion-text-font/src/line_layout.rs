//! R50.12 §5.37.6 — multi-line shaped paragraph layout (wrap + shape + stack).
//!
//! The last pure-CPU layer of the self-hosted text engine before paint wiring:
//! it joins the two halves built up to here — the UAX #14 greedy line breaker
//! ([`crate::wrap_paragraph`], §5.37.7) and the UAX #9 paragraph shaper
//! ([`crate::shape_paragraph`], §5.37.6) — into a wrapped, shaped block of
//! lines, each carrying its glyphs and a baseline.
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
use crate::paragraph::shape_runs_visual;
use crate::shape::PositionedGlyph;
use crate::wrap::{LineRange, wrap_paragraph};
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

    // Vertical metrics from hhea, guarded like `shape_run` (a degenerate
    // units-per-em or a non-finite / non-positive size collapses to zero).
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
    for range in &line_ranges {
        let clipped = clip_runs(&para_runs, range.start, range.end);
        let shaped = shape_runs_visual(font, paragraph, px_per_em, &clipped);
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
    use super::{ShapedLines, layout_paragraph};
    use crate::Font;
    use crate::shape::shape_run;
    use crate::shape_paragraph;
    use pinion_text_unicode::{is_removed_by_x9, reorder_visual, resolved_levels};

    const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
    const PX: f32 = 32.0;

    fn font() -> Font {
        let bytes = std::fs::read(NOTO).expect("read font fixture");
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
}
