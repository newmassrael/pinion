//! R50.11 §5.37.7 — greedy line breaking (width-driven line layout).
//!
//! Wires the UAX #14 break-opportunity analysis
//! ([`pinion_text_unicode::line_break_opportunities`], §5.37.7) into the shaping
//! path: given a paragraph, a font, a size, and a maximum line width, split the
//! paragraph into logical lines so each line is as full as possible without
//! exceeding the width — the classic greedy (first-fit) line breaker.
//!
//! Two entry points share one greedy core, differing only in how each segment's
//! advance is measured: [`wrap_paragraph`] measures in a single font,
//! [`wrap_paragraph_with_fallback`] measures across a font stack with
//! per-codepoint fallback (§5.37.10) so a mixed-script paragraph wraps at its
//! true advances rather than the primary font's `.notdef` widths.
//!
//! R1344 §5.41 — that core is no longer here. Both entry points are now thin
//! `measure`-closure wrappers over
//! [`pinion_text_unicode::wrap_paragraph_with_measure`], which owns the UAX #14
//! greedy breaker for every measurement model. The lift's forcing consumer was
//! the TUI backend (`pinion_tui::text_layout`), which wraps in terminal cells:
//! break opportunities are font-independent, so a second breaker would have been
//! pure duplication. This module keeps only the font-specific half — how wide a
//! segment is — plus the re-export of [`LineRange`] for existing callers.
//!
//! Lines are byte ranges in **logical** order. Visual reordering and per-line
//! shaping are a later step ([`crate::shape_paragraph`] applied per line, with
//! paragraph-context BIDI levels); the line *width* used here is the sum of
//! advances, which is reorder-invariant, so the break points are correct
//! regardless of direction.
//!
//! The breaker's own behaviour and its honest deferrals (trailing-whitespace
//! hang, Knuth-Plass) are documented with the breaker — see
//! [`pinion_text_unicode::wrap`]. Production **pixel** paint is still §5.36
//! parley/swash, so these two entry points remain a test forcing-consumer until
//! the paint path wires the self-hosted layout; the cell backend
//! (`pinion_tui::text_layout`) is the breaker's first production consumer.

use crate::Font;
use crate::fallback::shape_with_fallback;
use crate::shape::shape_run;
use pinion_text_unicode::wrap_paragraph_with_measure;

/// R1344 §5.37.7 — re-export of the lifted line range so existing
/// `pinion_text_font::LineRange` callers are unaffected by the move to
/// [`pinion_text_unicode`].
pub use pinion_text_unicode::LineRange;

/// Greedily break `paragraph` into lines no wider than `max_width` (device px at
/// `px_per_em`), at UAX #14 opportunities (§5.37.7). Returns the logical line
/// ranges, contiguous and covering the whole paragraph; an empty paragraph
/// yields no lines. A mandatory break always ends a line; a segment with no
/// interior opportunity that exceeds `max_width` is emitted as its own line.
#[must_use]
pub fn wrap_paragraph(
    font: &Font,
    paragraph: &str,
    px_per_em: f32,
    max_width: f32,
) -> Vec<LineRange> {
    wrap_paragraph_with_measure(paragraph, max_width, |seg| {
        shape_run(font, seg, px_per_em).advance
    })
}

/// Greedily break `paragraph` into lines no wider than `max_width`, measuring
/// each segment across the `fonts` stack with per-codepoint fallback
/// ([`shape_with_fallback`], §5.37.10) — the multi-font analogue of
/// [`wrap_paragraph`]. A mixed-script paragraph wraps at its *true* advances
/// (a codepoint the primary font lacks is measured in the font that actually
/// shapes it, not as the primary's `.notdef`), so the break points feed a
/// fallback-aware line layout ([`crate::line_layout::layout_paragraph_with_fallback`]).
/// A one-element stack reproduces [`wrap_paragraph`] exactly (one font run per
/// segment). An empty stack yields no lines (nothing can be laid out), matching
/// [`crate::line_layout::layout_paragraph_with_fallback`]. Kerning is dropped at
/// every break opportunity AND at every intra-segment font-run boundary (each
/// font run is shaped independently by [`shape_with_fallback`]), so the
/// fallback path's width under-estimate is slightly larger than the single-font
/// [`wrap_paragraph`]'s; both are sub-pixel for typical fonts and exact at the
/// chosen break.
#[must_use]
pub fn wrap_paragraph_with_fallback(
    fonts: &[&Font],
    paragraph: &str,
    px_per_em: f32,
    max_width: f32,
) -> Vec<LineRange> {
    if fonts.is_empty() {
        return Vec::new();
    }
    wrap_paragraph_with_measure(paragraph, max_width, |seg| {
        shape_with_fallback(fonts, seg, px_per_em).advance
    })
}

#[cfg(test)]
mod tests {
    use super::{LineRange, wrap_paragraph, wrap_paragraph_with_fallback};
    use crate::Font;
    use crate::fallback::shape_with_fallback;
    use crate::shape::shape_run;

    const NOTO: &str = "tests/fonts/NotoSans-Regular.ttf";
    const NANUM: &str = "tests/fonts/NanumGothic-Regular.ttf";
    const PX: f32 = 32.0;

    fn font() -> Font {
        load(NOTO)
    }

    fn load(path: &str) -> Font {
        let bytes = std::fs::read(path).expect("read font fixture");
        Font::from_bytes(bytes).expect("valid Font")
    }

    fn width(f: &Font, text: &str) -> f32 {
        shape_run(f, text, PX).advance
    }

    /// Lines must tile the paragraph: contiguous, gap-free, covering [0, len).
    fn assert_tiles(lines: &[LineRange], len: usize) {
        if lines.is_empty() {
            assert_eq!(len, 0, "only an empty paragraph yields no lines");
            return;
        }
        assert_eq!(lines[0].start, 0, "first line starts at 0");
        assert_eq!(lines[lines.len() - 1].end, len, "last line ends at len");
        for pair in lines.windows(2) {
            assert_eq!(
                pair[0].end, pair[1].start,
                "lines contiguous, no gap/overlap"
            );
            assert!(pair[0].start < pair[0].end, "line non-empty");
        }
    }

    #[test]
    fn empty_paragraph_yields_no_lines() {
        assert!(wrap_paragraph(&font(), "", PX, 1000.0).is_empty());
    }

    #[test]
    fn unbounded_width_is_a_single_line() {
        let f = font();
        let text = "the quick brown fox";
        let lines = wrap_paragraph(&f, text, PX, f32::MAX);
        assert_eq!(
            lines,
            vec![LineRange {
                start: 0,
                end: text.len()
            }]
        );
        assert_tiles(&lines, text.len());
    }

    #[test]
    fn breaks_between_words_at_a_width() {
        // "aaa bbb ccc": opportunities after each space (offsets 4, 8) + eot 11.
        // Pick a width that holds one "word " but not two, forcing a break at
        // each space — derived from measured advances, not a hardcoded pixel.
        let f = font();
        let text = "aaa bbb ccc";
        let two_words = width(&f, "aaa bbb "); // wider than one "aaa "
        let one_word = width(&f, "aaa ");
        let max = f32::midpoint(one_word, two_words); // holds one word, not two
        assert!(
            one_word <= max && two_words > max,
            "threshold brackets one word"
        );
        let lines = wrap_paragraph(&f, text, PX, max);
        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 4 },
                LineRange { start: 4, end: 8 },
                LineRange { start: 8, end: 11 },
            ]
        );
        assert_tiles(&lines, text.len());
    }

    #[test]
    fn mandatory_break_ends_a_line_under_unbounded_width() {
        // U+2028 LINE SEPARATOR is a UAX #14 mandatory break: it must split the
        // line even though the whole text fits the width.
        let f = font();
        let text = "ab\u{2028}cd";
        let lines = wrap_paragraph(&f, text, PX, f32::MAX);
        assert_eq!(lines.len(), 2, "mandatory break forces two lines");
        assert_eq!(lines[0].start, 0);
        assert_eq!(lines[1].end, text.len());
        assert_tiles(&lines, text.len());
    }

    #[test]
    fn overflowing_word_is_emitted_on_its_own_line() {
        // A single long token with no interior opportunity, narrower budget than
        // the token: it must be emitted (overflow), never loop or drop content.
        let f = font();
        let text = "wwwwwwwwww short";
        let big = width(&f, "wwwwwwwwww");
        let lines = wrap_paragraph(&f, text, PX, big / 2.0);
        assert_tiles(&lines, text.len());
        assert!(
            lines.len() >= 2,
            "the long token and the tail land on lines"
        );
        // First line is the overflowing token's segment — up to the first break
        // opportunity, which falls *after* the space (offset 11), so the line is
        // "wwwwwwwwww " and the tail "short" follows.
        assert_eq!(lines[0], LineRange { start: 0, end: 11 });
    }

    #[test]
    fn mandatory_overflow_breaks_at_earlier_fit_first() {
        // An optional break that fits, followed by a mandatory break whose
        // segment overflows: the greedy must break at the optional fit FIRST,
        // then let the mandatory close the tail — not emit an overflowing
        // mandatory line. Guards the branch order (width check before
        // mandatory-ness).
        let f = font();
        let text = "aa bb\u{2028}cc"; // optional after "aa ", mandatory at U+2028
        let max = f32::midpoint(width(&f, "aa "), width(&f, "aa bb")); // holds "aa " only
        let lines = wrap_paragraph(&f, text, PX, max);
        assert_tiles(&lines, text.len());
        // "aa " | "bb\u{2028}" | "cc": optional break taken before the mandatory.
        assert_eq!(
            lines[0],
            LineRange { start: 0, end: 3 },
            "break at optional fit"
        );
        assert_eq!(lines.len(), 3, "then mandatory closes the remainder");
    }

    #[test]
    fn consecutive_breaks_yield_no_zero_width_line() {
        // Adjacent mandatory breaks (blank line) must each be a 1-byte line, not
        // a zero-width line — pins the strictly-increasing-offset contract this
        // module depends on from line_break_opportunities.
        let f = font();
        for text in ["a\n\nb", "\n", "\n\n"] {
            let lines = wrap_paragraph(&f, text, PX, f32::MAX);
            assert_tiles(&lines, text.len());
        }
        // "a\n\nb": lines [0,2) "a\n", [2,3) "\n" (blank), [3,4) "b".
        let lines = wrap_paragraph(&f, "a\n\nb", PX, f32::MAX);
        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 2 },
                LineRange { start: 2, end: 3 },
                LineRange { start: 3, end: 4 },
            ]
        );
    }

    #[test]
    fn every_non_overflow_line_fits_the_width() {
        use pinion_text_unicode::line_break_opportunities;
        // Invariant: a line may exceed the width ONLY if it has no interior break
        // opportunity (a forced single segment). Cross-checked against the actual
        // opportunities, not a space search.
        let f = font();
        let text = "alpha beta gamma delta epsilon zeta";
        let max = width(&f, "alpha beta "); // a couple of words
        let lines = wrap_paragraph(&f, text, PX, max);
        assert_tiles(&lines, text.len());
        let opps = line_break_opportunities(text);
        for line in &lines {
            let w = width(&f, &text[line.start..line.end]);
            let interior = opps
                .iter()
                .filter(|o| o.offset > line.start && o.offset < line.end)
                .count();
            // +1px absorbs the disclosed cross-opportunity kerning under-estimate.
            assert!(
                w <= max + 1.0 || interior == 0,
                "line {line:?} width {w} > {max} yet has {interior} interior break(s)"
            );
        }
    }

    // ---- wrap_paragraph_with_fallback (R1054 multi-font wrapping) ----

    #[test]
    fn fallback_single_font_stack_equals_wrap_paragraph() {
        // A one-element stack measures each segment in that one font (one font run
        // per segment), so the fallback wrap must reproduce the single-font wrap
        // byte-for-byte — the special case that keeps the two paths consistent.
        let f = font();
        let cases: [(&str, f32); 3] = [
            ("the quick brown fox", f32::MAX),
            ("alpha beta gamma delta", width(&f, "alpha beta ")),
            ("a\n\nb\u{2028}c", f32::MAX),
        ];
        for (text, max) in cases {
            assert_eq!(
                wrap_paragraph_with_fallback(&[&f], text, PX, max),
                wrap_paragraph(&f, text, PX, max),
                "1-font stack == single-font wrap for {text:?}",
            );
        }
    }

    /// Hangul GA / NA / DA (U+AC00 U+B098 U+B2E4) — uncovered by `NotoSans`,
    /// covered by `NanumGothic`; written escaped to keep the source ASCII-only.
    const HANGUL: &str = "\u{AC00}\u{B098}\u{B2E4}";

    #[test]
    fn fallback_wrap_measures_hangul_in_the_cjk_font() {
        // The Hangul word is uncovered by Noto (it would measure as Noto's .notdef
        // box) but real in Nanum. Wrapping must measure it in the font that
        // actually shapes it, so the break responds to the *true* Hangul advance,
        // not the primary font's placeholder width.
        let noto = font();
        let nanum = load(NANUM);
        let stack = [&noto, &nanum];
        let prefix_text = format!("aa {HANGUL} "); // "aa <GA NA DA> " = 13 bytes
        let text = format!("aa {HANGUL} bb");

        // Guard the premise: the Hangul genuinely measures differently in the two
        // models (also fails loudly if Noto ever starts covering Hangul).
        let in_nanum = shape_with_fallback(&stack, HANGUL, PX).advance;
        let in_noto = shape_run(&noto, HANGUL, PX).advance;
        assert!(
            (in_nanum - in_noto).abs() > 1.0,
            "Hangul advance must differ: Nanum {in_nanum} vs Noto .notdef {in_noto}",
        );

        // At a budget tuned to the TRUE width of "aa <Hangul> ", the Hangul word
        // fits line 0 and only the trailing "bb" wraps.
        let prefix = shape_with_fallback(&stack, &prefix_text, PX).advance;
        let fits = wrap_paragraph_with_fallback(&stack, &text, PX, prefix + 1.0);
        assert_tiles(&fits, text.len());
        assert_eq!(
            fits[0],
            LineRange {
                start: 0,
                end: prefix_text.len(),
            },
            "true budget holds the whole Hangul word on line 0",
        );
        assert_eq!(fits.len(), 2, "trailing 'bb' wraps to a second line");

        // Just over "aa " only: the Hangul word no longer fits its true advance and
        // wraps off line 0 — the break tracks the real measurement, not a constant.
        let tight = shape_with_fallback(&stack, "aa ", PX).advance + 1.0;
        let wrapped = wrap_paragraph_with_fallback(&stack, &text, PX, tight);
        assert_tiles(&wrapped, text.len());
        assert_eq!(
            wrapped[0],
            LineRange { start: 0, end: 3 },
            "under the Hangul's true width, only 'aa ' fits line 0",
        );
    }
}
