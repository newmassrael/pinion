//! R1344 §5.37.7 §5.41 — measurement-agnostic greedy line breaking.
//!
//! The width-driven half of line layout, factored out of the font model. Given a
//! paragraph, a maximum line width, and a closure that measures a text segment's
//! width, split the paragraph into logical lines so each is as full as possible
//! without exceeding the width — the classic greedy (first-fit) breaker over
//! [`crate::line_break_opportunities`] (UAX #14, §5.37.7).
//!
//! ## Why this lives in `pinion-text-unicode`, not `pinion-text-font`
//!
//! UAX #14 break *opportunities* are a property of the text, not of the font: the
//! only font-dependent input to a greedy breaker is how wide a segment measures.
//! R1344 §5.41 surfaced the third consumer that makes this concrete — the TUI
//! backend wraps in **terminal cells** (`unicode-width`), where "shaping" is
//! meaningless but line breaking is not. Keeping the breaker here (a crate with
//! zero external dependencies) lets the cell backend reuse the exact same UAX #14
//! decisions the pixel backend uses, with only the measure closure differing:
//!
//! | consumer | `measure` | unit |
//! |---|---|---|
//! | `pinion_text_font::wrap_paragraph` | `shape_run(font, seg, px).advance` | device px |
//! | `pinion_text_font::wrap_paragraph_with_fallback` | `shape_with_fallback(fonts, seg, px).advance` | device px |
//! | `pinion_tui::text_layout::CellTextLayout` | `cell_width(seg)` | terminal cells |
//!
//! Before R1344 the seam already existed *inside* `pinion-text-font::wrap` (two
//! entry points differing only by their measure closure) but was private, so the
//! TUI paint walker hand-rolled a grapheme loop that did not wrap at all
//! (§5.41's `TextAlign` wrap-policy open item). Exposing the seam resolves that
//! item without a second breaker.
//!
//! ## Honest deferrals (they moved here with the breaker)
//!
//! * **Trailing-whitespace hang** — the UAX #14 width refinement that lets a
//!   line's trailing spaces exceed the width. Not applied, so `"alpha beta "`
//!   (11 units) does not fit a 10-unit budget and breaks after `"alpha "`.
//! * **Knuth-Plass optimal breaking** — this is first-fit greedy; no
//!   whole-paragraph badness minimisation.
//!
//! Both are properties of the breaker, so every consumer inherits them
//! identically — which is the point of there being one breaker.
//!
//! ## Line ranges include their trailing break codepoint
//!
//! A [`LineRange`] produced by a mandatory break spans *through* the break
//! character (`"a\nb"` → `[0,2)` = `"a\n"`, `[2,3)` = `"b"`). Renderers that
//! cannot draw a control character — the terminal backend — must trim it; see
//! [`trim_trailing_break`].

use crate::linebreak::{BreakOpportunity, LineBreak, line_break_class, line_break_opportunities};

/// One laid-out line: a byte range `start..end` (`end` exclusive) into the
/// source paragraph, in logical order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineRange {
    /// Byte offset of the line's first codepoint.
    pub start: usize,
    /// Byte offset just past the line's last codepoint.
    pub end: usize,
}

/// Greedily break `paragraph` into lines no wider than `max_width`, at UAX #14
/// opportunities (§5.37.7), measuring each inter-opportunity segment with
/// `measure`.
///
/// `max_width` and `measure`'s return share one caller-chosen unit (device px
/// for the font backends, cells for the TUI backend). Returns the logical line
/// ranges, contiguous and covering the whole paragraph; an empty paragraph
/// yields no lines. A mandatory break always ends a line; a segment with no
/// interior opportunity that exceeds `max_width` is emitted as its own
/// (overflow) line rather than looping.
///
/// Lines are byte ranges in **logical** order. The line *width* is the sum of
/// segment measures, which is reorder-invariant, so the break points are correct
/// regardless of BIDI direction.
#[must_use]
pub fn wrap_paragraph_with_measure(
    paragraph: &str,
    max_width: f32,
    measure: impl Fn(&str) -> f32,
) -> Vec<LineRange> {
    let breaks = line_break_opportunities(paragraph);
    if breaks.is_empty() {
        return Vec::new();
    }
    let adv_to = cumulative_advances(paragraph, &breaks, measure);
    greedy_break(&breaks, &adv_to, max_width)
}

/// Whether `c` is a UAX #14 mandatory break — the *same* classification LB4 /
/// LB5 use to set [`BreakOpportunity::mandatory`].
///
/// Derived from [`line_break_class`] (the generated UCD table) rather than a
/// hand-listed set of codepoints. That matters: the breaker ends a line from
/// the table, so a trimmer working off a literal list would silently disagree
/// the moment the table gains a codepoint — and a disagreement here means a
/// mandatory-break codepoint survives into a rendered line, which on a cell
/// backend is a raw control byte written to the terminal.
#[must_use]
fn is_mandatory_break(c: char) -> bool {
    matches!(
        line_break_class(c),
        LineBreak::BK | LineBreak::CR | LineBreak::LF | LineBreak::NL
    )
}

/// Trim the trailing UAX #14 mandatory-break codepoint(s) from `line`, yielding
/// the printable slice.
///
/// [`wrap_paragraph_with_measure`] emits line ranges that span through their
/// terminating break character (see the module docs). A glyph rasterizer can
/// shape that codepoint harmlessly (it inks nothing), but a **cell** backend
/// would write a raw control byte into the terminal stream, so the TUI trims.
/// Handles the CRLF digraph as one break (LB5 `CR × LF`).
///
/// Non-break control characters (`\t`, other C0) are NOT trimmed here — they are
/// interior to the line and are the width/paint layer's concern.
#[must_use]
pub fn trim_trailing_break(line: &str) -> &str {
    let Some(last) = line.chars().next_back() else {
        return line;
    };
    if !is_mandatory_break(last) {
        return line;
    }
    let trimmed = &line[..line.len() - last.len_utf8()];
    // LB5 CR × LF — the digraph is ONE break, so drop the LF's CR partner too.
    if last == '\n'
        && let Some(rest) = trimmed.strip_suffix('\r')
    {
        return rest;
    }
    trimmed
}

/// Cumulative advance to each break offset, segment by segment. `adv_to[i]` is
/// the summed measure of `paragraph[0..breaks[i].offset]`, each inter-break
/// segment measured once by `measure` (O(n) measurement). For a shaping model,
/// kerning is dropped at every interior opportunity within a line (each segment
/// is measured independently), so the summed width is a slight under-estimate vs
/// a single-run shape; for typical fonts this is sub-pixel, and at the chosen
/// break point it is exact. A cell model has no kerning, so its sum is exact.
fn cumulative_advances(
    paragraph: &str,
    breaks: &[BreakOpportunity],
    measure: impl Fn(&str) -> f32,
) -> Vec<f32> {
    let mut adv_to: Vec<f32> = Vec::with_capacity(breaks.len());
    let mut seg_start = 0usize;
    let mut cumulative = 0.0_f32;
    for bp in breaks {
        cumulative += measure(&paragraph[seg_start..bp.offset]);
        adv_to.push(cumulative);
        seg_start = bp.offset;
    }
    adv_to
}

/// The greedy (first-fit) line breaker — the single SSOT across every measure
/// model. `adv_to[i]` is the cumulative advance at `breaks[i]`; a line's width is
/// `adv_to[i] - base`. The width check precedes the mandatory-ness check so an
/// optional break that fits is taken before a following mandatory break whose
/// segment would overflow.
fn greedy_break(breaks: &[BreakOpportunity], adv_to: &[f32], max_width: f32) -> Vec<LineRange> {
    let mut lines: Vec<LineRange> = Vec::new();
    let mut line_start = 0usize;
    let mut base = 0.0_f32; // cumulative advance at the current line's start
    let mut last_fit: Option<usize> = None; // break index that fits the current line
    let mut i = 0usize;
    while i < breaks.len() {
        let bp = breaks[i];
        let width = adv_to[i] - base;
        if width <= max_width {
            if bp.mandatory {
                lines.push(LineRange {
                    start: line_start,
                    end: bp.offset,
                });
                line_start = bp.offset;
                base = adv_to[i];
                last_fit = None;
            } else {
                last_fit = Some(i);
            }
            i += 1;
        } else if let Some(fit) = last_fit {
            // Overflow: end the line at the last fitting opportunity, then
            // re-examine the current break on the next line.
            lines.push(LineRange {
                start: line_start,
                end: breaks[fit].offset,
            });
            line_start = breaks[fit].offset;
            base = adv_to[fit];
            last_fit = None;
        } else {
            // Overflow with no earlier opportunity: emit this segment alone.
            lines.push(LineRange {
                start: line_start,
                end: bp.offset,
            });
            line_start = bp.offset;
            base = adv_to[i];
            last_fit = None;
            i += 1;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{LineRange, trim_trailing_break, wrap_paragraph_with_measure};

    /// A measure model with no font at all: one unit per `char`. Enough to pin
    /// the breaker's decisions independently of any shaping crate.
    fn chars(seg: &str) -> f32 {
        // Break codepoints occupy no width in this model, mirroring the cell
        // backend (a terminal never draws them).
        let n = seg.chars().filter(|c| !c.is_control()).count();
        #[allow(
            clippy::cast_precision_loss,
            reason = "test paragraphs are a handful of chars — exact in f32"
        )]
        let w = n as f32;
        w
    }

    fn wrap(text: &str, max: f32) -> Vec<LineRange> {
        wrap_paragraph_with_measure(text, max, chars)
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
        assert!(wrap("", 10.0).is_empty());
    }

    #[test]
    fn unbounded_width_is_a_single_line() {
        let text = "the quick brown fox";
        assert_eq!(
            wrap(text, f32::MAX),
            vec![LineRange {
                start: 0,
                end: text.len()
            }]
        );
    }

    #[test]
    fn breaks_between_words_at_a_width() {
        // "aaa bbb ccc": opportunities after each space (offsets 4, 8) + eot 11.
        // Width 4 holds exactly one "aaa " but not "aaa bbb ".
        let text = "aaa bbb ccc";
        let lines = wrap(text, 4.0);
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
        let text = "ab\u{2028}cd";
        let lines = wrap(text, f32::MAX);
        assert_eq!(lines.len(), 2, "mandatory break forces two lines");
        assert_tiles(&lines, text.len());
    }

    #[test]
    fn overflowing_token_is_emitted_on_its_own_line() {
        let text = "wwwwwwwwww short";
        let lines = wrap(text, 5.0);
        assert_tiles(&lines, text.len());
        assert_eq!(
            lines[0],
            LineRange { start: 0, end: 11 },
            "the un-breakable token overflows on its own line",
        );
    }

    #[test]
    fn mandatory_overflow_breaks_at_earlier_fit_first() {
        // Optional break that fits, then a mandatory break whose segment
        // overflows: the optional fit must win first. Guards the branch order.
        let text = "aa bb\u{2028}cc";
        let lines = wrap(text, 3.0);
        assert_tiles(&lines, text.len());
        assert_eq!(
            lines[0],
            LineRange { start: 0, end: 3 },
            "break at optional fit"
        );
        assert_eq!(lines.len(), 3, "then mandatory closes the remainder");
    }

    #[test]
    fn consecutive_breaks_yield_no_zero_width_line() {
        for text in ["a\n\nb", "\n", "\n\n"] {
            assert_tiles(&wrap(text, f32::MAX), text.len());
        }
        assert_eq!(
            wrap("a\n\nb", f32::MAX),
            vec![
                LineRange { start: 0, end: 2 },
                LineRange { start: 2, end: 3 },
                LineRange { start: 3, end: 4 },
            ]
        );
    }

    // ---- trim_trailing_break ----

    #[test]
    fn trims_every_uax14_mandatory_break_form() {
        for (line, want) in [
            ("a\n", "a"),
            ("a\r\n", "a"),
            ("a\r", "a"),
            ("a\u{000B}", "a"),
            ("a\u{000C}", "a"),
            ("a\u{0085}", "a"),
            ("a\u{2028}", "a"),
            ("a\u{2029}", "a"),
            ("a", "a"),
            ("", ""),
        ] {
            assert_eq!(trim_trailing_break(line), want, "trimming {line:?}");
        }
    }

    #[test]
    fn trim_agrees_with_the_breaker_on_what_a_mandatory_break_is() {
        // The coupling that must never drift: the breaker ends a line at a
        // mandatory break (LB4/LB5, off the UCD table) and the trimmer strips
        // that same codepoint. If the two ever disagree, a break codepoint
        // survives into a rendered line — a raw control byte on a cell
        // backend. Asserted against the breaker's OWN verdict, not a list.
        for c in [
            '\n', '\r', '\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}',
        ] {
            let text = format!("a{c}b");
            let lines = wrap(&text, f32::MAX);
            assert_eq!(lines.len(), 2, "the breaker treats {c:?} as mandatory");
            let first = &text[lines[0].start..lines[0].end];
            assert_eq!(
                trim_trailing_break(first),
                "a",
                "the trimmer must strip the same {c:?} the breaker broke on",
            );
        }
    }

    #[test]
    fn trim_keeps_interior_breaks_and_non_break_controls() {
        // Only the TRAILING break is a line terminator. A tab is interior.
        assert_eq!(trim_trailing_break("a\tb"), "a\tb");
        assert_eq!(trim_trailing_break("a\nb"), "a\nb");
    }

    #[test]
    fn every_wrapped_line_is_trimmable_to_printable_text() {
        // The composition the TUI relies on: wrap, then trim each line, and no
        // printable content is lost.
        let text = "alpha beta\ngamma delta";
        let lines = wrap(text, 6.0);
        assert_tiles(&lines, text.len());
        let rebuilt: String = lines
            .iter()
            .map(|l| trim_trailing_break(&text[l.start..l.end]))
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            !rebuilt.contains('\n'),
            "no break survives the trim: {rebuilt:?}"
        );
        for word in ["alpha", "beta", "gamma", "delta"] {
            assert!(rebuilt.contains(word), "{word} survived: {rebuilt:?}");
        }
    }
}
