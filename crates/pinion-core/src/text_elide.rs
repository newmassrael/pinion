//! R1654 §5.36 §2 #6 — **which characters survive when a string does not fit.**
//!
//! [`TextOverflow::Ellipsis`] has been on
//! the wire, in the footprint census and readable back off a text field since
//! R47.5. Nothing implemented it. Both painters carried the same note — the
//! GPU adapter's *"Clip + Ellipsis (silent fallback to Clip until R47.x
//! ellipsis pass)"* and the terminal walker's *"truncate. R51.111+ adds the
//! ellipsis policy"* — so a caller asking for an ellipsis got a hard cut, and
//! the two are not the same answer: a cut leaves no evidence that anything was
//! removed, which is precisely the fact a person reading a truncated endpoint
//! needs.
//!
//! # Why the policy is here and the measurement is not
//!
//! The question "how wide is this string" has two legitimate answers in this
//! project — shaped pixels on the GPU path, terminal cells on the §2 #6 dual —
//! and they cannot be reconciled: a proportional font's advance is not a whole
//! number of cells and never will be. What CAN be shared, and what must be
//! shared or the two backends drift, is the *policy*: which end gives way, how
//! much of it, and what marks the cut.
//!
//! So this module takes the metric as a closure and the segmentation as a list
//! of byte offsets the caller considers cuttable. The GPU path hands over
//! parley's cluster boundaries and an advance read off an already-shaped line;
//! the terminal hands over grapheme boundaries and a cell count. Neither
//! re-implements the decision.
//!
//! # What the reference does, measured
//!
//! Its metrics class exposes `elidedText(text, mode, width)` with four modes,
//! and that is a **helper the caller must remember to call**: measured on 6.11,
//! its label class has no elide property at all (only its item views do, and
//! theirs defaults to eliding at the end), so a label handed a string too wide
//! for it simply clips — its size hint still reports the full natural width.
//! And the elided string is not observable afterwards: `text()` returns what
//! was authored, so nothing outside the paint call can say what a reader
//! actually saw.
//!
//! Here it is a property of the text itself, so every run has it rather than
//! the ones whose widget author remembered; and the result is published
//! (`scene/text_painted`), because a screen that shows `demo/…` while its scene
//! reports `demo/units/1/pose` is a screen that lies to §2 #7.

use crate::style::TextOverflow;

/// The character that marks a cut. U+2026, which is also what the reference
/// toolkit's metrics helper inserts (measured, not assumed).
pub const ELLIPSIS: &str = "\u{2026}";

/// What [`elide_to_fit`] needs to know about a caller's text.
///
/// Grouped into a struct rather than four parameters because three of the four
/// are only meaningful together: a boundary list belongs to one string, and a
/// measure that disagrees with that string measures nothing.
pub struct ElideRequest<'a> {
    /// The authored string.
    pub content: &'a str,
    /// Ascending UTF-8 byte offsets this caller is willing to cut at, `0` and
    /// `content.len()` included. Parley cluster edges on the GPU path; grapheme
    /// boundaries on the terminal one. A caller that offered `char_indices`
    /// would be offering to split a combining mark from its base.
    pub boundaries: &'a [usize],
    /// The width the result has to fit inside, in whatever unit `measure`
    /// answers in.
    pub budget: u32,
}

/// R1654 — the string that is painted when `content` does not fit `budget`.
///
/// Returns `None` when nothing needs to change: the policy does not elide, or
/// the whole string already fits. `Some` carries the replacement, which always
/// contains [`ELLIPSIS`] — a caller can therefore tell "was elided" from "was
/// not" without measuring anything a second time.
///
/// `measure` is called O(log n) times over the boundary list, plus at most two
/// steps of back-off: the search assumes the metric is monotone in the prefix,
/// which shaped advances are not *quite* (a kerning pair can be narrower than
/// its left member alone), so the result is verified and walked back rather
/// than trusted.
///
/// # The degenerate end, stated
///
/// When not even [`ELLIPSIS`] fits the budget, the answer is [`ELLIPSIS`]
/// alone. Returning the empty string would be the other defensible choice and
/// is rejected: a box too narrow for one character is a layout defect, and a
/// box that renders *nothing* hides it while a box showing a lone ellipsis
/// shows exactly where it is.
pub fn elide_to_fit(
    request: &ElideRequest<'_>,
    overflow: TextOverflow,
    measure: &mut dyn FnMut(&str) -> u32,
) -> Option<Elision> {
    let side = ElideSide::of(overflow)?;
    let content = request.content;
    if content.is_empty() || measure(content) <= request.budget {
        return None;
    }
    let cuts = usable_boundaries(content, request.boundaries);
    let budget = request.budget;
    let fits = |candidate: &str, measure: &mut dyn FnMut(&str) -> u32| measure(candidate) <= budget;

    // The largest k for which the k-th candidate fits. `cuts` is ascending and
    // the candidates grow with k, so the predicate is (near enough) monotone
    // and a binary search finds the edge in log n measurements.
    let build = |k: usize| side.candidate(content, &cuts, k);
    let mut lo = 0usize;
    let mut hi = cuts.len().saturating_sub(1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(&build(mid).text, measure) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut best = build(lo);
    // The back-off the non-monotone metric earns. Bounded, because an unbounded
    // loop here would be a measure that never agrees with the search.
    for _ in 0..2 {
        if fits(&best.text, measure) || lo == 0 {
            break;
        }
        lo -= 1;
        best = build(lo);
    }
    Some(best)
}

/// What [`elide_to_fit`] decided: the string to paint, and which bytes of the
/// original survived into it.
///
/// The ranges are carried rather than left to be re-derived because a caller
/// with per-span styling has to move those spans onto the new string, and
/// recovering "which bytes are these" from the result means comparing two
/// strings that differ in the middle — an inference where the decision is
/// already known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elision {
    /// The string a painter draws.
    pub text: String,
    /// How many bytes of the original are kept at the FRONT of `text`.
    pub head: usize,
    /// How many bytes of the original are kept at the END of `text`.
    pub tail: usize,
}

impl Elision {
    /// Move a byte range on the original string onto [`Self::text`], or `None`
    /// when the elision removed all of it.
    ///
    /// The one operation a styled-run consumer needs, so the arithmetic that
    /// relates the two strings lives beside the decision that produced them.
    #[must_use]
    pub fn remap(&self, original_len: usize, start: usize, end: usize) -> Option<(usize, usize)> {
        let kept_tail_from = original_len.saturating_sub(self.tail);
        let ellipsis_len = ELLIPSIS.len();
        // The head keeps its offsets; the tail shifts by however much the
        // removed middle differs from the ellipsis that replaced it.
        let head_hit = (start.min(self.head), end.min(self.head));
        if head_hit.0 < head_hit.1 {
            return Some(head_hit);
        }
        let tail_start = start.max(kept_tail_from);
        let tail_end = end.max(kept_tail_from);
        if tail_start < tail_end {
            let shift = kept_tail_from - self.head - ellipsis_len;
            return Some((tail_start - shift, tail_end - shift));
        }
        None
    }
}

/// Which end of the string gives way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElideSide {
    Start,
    Middle,
    End,
}

impl ElideSide {
    /// The eliding arms of [`TextOverflow`], and `None` for the arms that do
    /// not elide.
    ///
    /// Exhaustive over the enum on purpose, and inside the crate that owns it
    /// so the compiler accepts nothing less: a sixth arm has to be classified
    /// here rather than defaulting into "does not elide", which is the silent
    /// degradation this whole module exists to end.
    fn of(overflow: TextOverflow) -> Option<Self> {
        match overflow {
            TextOverflow::Visible | TextOverflow::Clip => None,
            TextOverflow::Ellipsis => Some(Self::End),
            TextOverflow::EllipsisStart => Some(Self::Start),
            TextOverflow::EllipsisMiddle => Some(Self::Middle),
        }
    }

    /// The `k`-th candidate: `k = 0` keeps nothing but the ellipsis, and
    /// `k = cuts.len() - 1` keeps as much as any candidate does.
    fn candidate(self, content: &str, cuts: &[usize], k: usize) -> Elision {
        let last = cuts.len().saturating_sub(1);
        let k = k.min(last);
        let (left, right) = match self {
            Self::End => (cuts[k], content.len()),
            Self::Start => (0, cuts[last - k]),
            Self::Middle => {
                // Half the kept characters from each end, the extra one going
                // to the front: reading a path, the head disambiguates more
                // than the tail.
                let head = k.div_ceil(2);
                let tail = k - head;
                (cuts[head], cuts[last - tail])
            }
        };
        if left >= right {
            return Elision {
                text: ELLIPSIS.to_owned(),
                head: 0,
                tail: 0,
            };
        }
        Elision {
            text: format!("{}{ELLIPSIS}{}", &content[..left], &content[right..]),
            head: left,
            tail: content.len() - right,
        }
    }
}

/// The caller's boundary list, made safe to index into `content`.
///
/// A caller is trusted for its segmentation and not for its bookkeeping: an
/// offset that is not a char boundary would panic the slice, and a list missing
/// its ends would silently refuse to keep the whole string.
fn usable_boundaries(content: &str, offered: &[usize]) -> Vec<usize> {
    let mut cuts: Vec<usize> = offered
        .iter()
        .copied()
        .filter(|b| *b <= content.len() && content.is_char_boundary(*b))
        .collect();
    cuts.push(0);
    cuts.push(content.len());
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One unit per char, which is what a monospace terminal measures and what
    /// makes an expected string readable in a test.
    fn per_char(s: &str) -> u32 {
        u32::try_from(s.chars().count()).unwrap_or(u32::MAX)
    }

    fn request<'a>(content: &'a str, budget: u32, cuts: &'a [usize]) -> ElideRequest<'a> {
        ElideRequest {
            content,
            boundaries: cuts,
            budget,
        }
    }

    fn all_cuts(content: &str) -> Vec<usize> {
        content
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(content.len()))
            .collect()
    }

    /// ★ Every arm, because an enum with arms is only covered when every arm
    /// has been exercised — the rule R1621 wrote after a desk fixture reached
    /// one of four.
    #[test]
    fn r1654_every_arm_of_the_vocabulary_is_answered() {
        let content = "demo/units/1/pose";
        let cuts = all_cuts(content);
        let mut answers = Vec::new();
        for overflow in TextOverflow::ALL {
            let out = elide_to_fit(&request(content, 8, &cuts), overflow, &mut |s| per_char(s));
            answers.push((overflow, out.map(|e| e.text)));
        }
        assert_eq!(
            answers,
            vec![
                (TextOverflow::Visible, None),
                (TextOverflow::Clip, None),
                (TextOverflow::Ellipsis, Some("demo/un\u{2026}".to_owned())),
                (
                    TextOverflow::EllipsisStart,
                    Some("\u{2026}/1/pose".to_owned())
                ),
                (
                    TextOverflow::EllipsisMiddle,
                    Some("demo\u{2026}ose".to_owned())
                ),
            ],
            "five arms, and the three that elide differ in WHICH characters survive"
        );
    }

    /// Whatever the arm and whatever the budget, the answer fits.
    #[test]
    fn r1654_the_answer_fits_the_budget_at_every_width() {
        let content = "demo/units/1/pose";
        let cuts = all_cuts(content);
        for overflow in [
            TextOverflow::Ellipsis,
            TextOverflow::EllipsisStart,
            TextOverflow::EllipsisMiddle,
        ] {
            for budget in 1..=u32::try_from(content.chars().count()).unwrap() {
                let out = elide_to_fit(&request(content, budget, &cuts), overflow, &mut |s| {
                    per_char(s)
                })
                .map(|e| e.text);
                let Some(out) = out else {
                    assert!(
                        per_char(content) <= budget,
                        "{overflow:?} at {budget}: refused to elide a string that does not fit"
                    );
                    continue;
                };
                assert!(
                    per_char(&out) <= budget,
                    "{overflow:?} at {budget}: {out:?} is {} wide",
                    per_char(&out)
                );
                assert!(out.contains(ELLIPSIS), "{overflow:?} at {budget}: {out:?}");
            }
        }
    }

    /// A string that fits is left alone — the answer says "nothing to do"
    /// rather than handing back a copy, so a caller can tell the two apart.
    #[test]
    fn r1654_a_string_that_fits_is_not_touched() {
        let content = "short";
        let cuts = all_cuts(content);
        for overflow in TextOverflow::ALL {
            assert_eq!(
                elide_to_fit(&request(content, 99, &cuts), overflow, &mut |s| per_char(s)),
                None,
                "{overflow:?}"
            );
        }
    }

    /// ★ The cut lands on a boundary the CALLER offered, never inside one.
    ///
    /// The case that matters is a combining mark: split from its base it
    /// becomes a different character, and a policy that cut at char offsets
    /// would do exactly that. Here the caller offers grapheme edges and the
    /// policy cannot cut anywhere else.
    #[test]
    fn r1654_a_cut_lands_only_where_the_caller_allows_one() {
        // "e" + U+0301 (combining acute) x3, one grapheme each.
        let content = "e\u{301}e\u{301}e\u{301}abc";
        let graphemes = [0usize, 3, 6, 9, 10, 11, 12];
        let out = elide_to_fit(
            &request(content, 3, &graphemes),
            TextOverflow::Ellipsis,
            &mut |s| {
                u32::try_from(s.chars().filter(|c| !matches!(c, '\u{301}')).count())
                    .unwrap_or(u32::MAX)
            },
        )
        .expect("it does not fit")
        .text;
        assert!(
            out.starts_with("e\u{301}") || out == ELLIPSIS,
            "the acute stayed with its base: {out:?}"
        );
        assert!(
            !out.starts_with('\u{301}'),
            "a bare combining mark escaped: {out:?}"
        );
    }

    /// ★ A styled span survives the elision, or is dropped, and the answer says
    /// which — the arithmetic a rich-text painter would otherwise infer by
    /// comparing two strings that differ in the middle.
    #[test]
    fn r1654_a_span_is_moved_onto_the_painted_string() {
        let content = "demo/units/1/pose";
        let cuts = all_cuts(content);
        let out = elide_to_fit(
            &request(content, 9, &cuts),
            TextOverflow::EllipsisMiddle,
            &mut |s| per_char(s),
        )
        .expect("it does not fit");
        let n = content.len();
        // A span over the kept head keeps its offsets.
        assert_eq!(out.remap(n, 0, 4), Some((0, 4)));
        // A span over the removed middle is gone.
        assert_eq!(out.remap(n, out.head, n - out.tail), None);
        // A span over the kept tail lands inside the painted string.
        let tail = out.remap(n, n - out.tail, n).expect("kept");
        assert_eq!(&out.text[tail.0..tail.1], &content[n - out.tail..]);
        assert!(tail.1 <= out.text.len(), "inside the painted string");
    }

    /// A budget below one character still answers, and the answer says
    /// something was removed.
    #[test]
    fn r1654_a_budget_too_small_for_one_character_still_marks_the_cut() {
        let content = "abcdef";
        let cuts = all_cuts(content);
        let out = elide_to_fit(
            &request(content, 0, &cuts),
            TextOverflow::Ellipsis,
            &mut |s| per_char(s),
        );
        assert_eq!(out.map(|e| e.text).as_deref(), Some(ELLIPSIS));
    }

    /// The metric is allowed to be non-monotone; the answer still fits.
    ///
    /// A kerning pair can measure narrower than its left member alone, which
    /// breaks the assumption a binary search rests on. The back-off is what
    /// covers it, and this is the fixture that exercises the back-off rather
    /// than asserting it exists.
    #[test]
    fn r1654_a_metric_with_a_kerning_dip_still_produces_a_fitting_answer() {
        let content = "AVAVAVAV";
        let cuts = all_cuts(content);
        // Every "AV" pair measures 1 instead of 2 — a 50% kern, far past
        // anything real, so the search's assumption is genuinely violated.
        let kerned = |s: &str| {
            let pairs = u32::try_from(s.matches("AV").count()).unwrap_or(0);
            u32::try_from(s.chars().count()).unwrap_or(u32::MAX) - pairs
        };
        for budget in 1..8 {
            let mut measure = |s: &str| kerned(s);
            let out = elide_to_fit(
                &request(content, budget, &cuts),
                TextOverflow::Ellipsis,
                &mut measure,
            );
            let Some(out) = out.map(|e| e.text) else {
                assert!(
                    kerned(content) <= budget,
                    "budget {budget}: refused to elide a string that does not fit"
                );
                continue;
            };
            assert!(
                kerned(&out) <= budget,
                "budget {budget}: {out:?} measures {}",
                kerned(&out)
            );
        }
    }
}
