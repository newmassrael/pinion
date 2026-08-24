//! R1672 §5.32 §5.45 — the **ink gate** a screen's integration test runs: is
//! every painted mark inside the box that owns it, measured with a stand-in
//! that does not depend on which fonts the host has?
//!
//! ## Why the framework owns this and not each screen
//!
//! [`crate::containment::escapes`] is the check, and it takes a *metric* —
//! because ink is not in the scene, it is a measurement. Every screen that runs
//! the check therefore has to supply one, and R1653 wrote a stand-in into one
//! screen, R1663 copied it into a second, and R1672 found a third copy inside
//! the first screen's own helper. Three mechanical copies of one rule is the
//! immediate-lift case, and the hazard is not the duplication: it is that two
//! screens measuring differently would **disagree about the same defect**, one
//! reporting a string as fitting its column and the other as escaping it.
//!
//! The sharper reason is what the copies were used for. Screen A ran
//! [`escapes`](crate::containment::escapes); screen B held a copy of the metric
//! and never called it, so a counterfactual that put screen B's three panes
//! back over the outlines of the panels holding them was caught by **nothing**.
//! A vocabulary that lives in one consumer is a vocabulary the other consumers
//! cannot speak ([[r1670-the-sweeps-red-demos-are-paid-off]]).
//!
//! ## Why the width is a stand-in and the height is not
//!
//! *"Is this string too long for its column"* is a question a font-independent
//! stand-in answers conservatively — it is wider per character than any face a
//! screen here uses, so a box that passes has room for a real one — and it is
//! the question an author gets wrong. *"Is this run's INK taller than its box"*
//! is answered by the layout pass from the host's real metrics, and re-deciding
//! it here against a constant would make the gate green or red depending on
//! which fonts are installed ([[zero-flake-policy]]).
//!
//! The shaped answer to both is what `scene/containment` reports at boot, on
//! the machine that is actually painting.
//!
//! ## ★★★★★ R1800 — and that paragraph is why the vertical question went
//! unasked for a hundred and thirty rounds
//!
//! It used to say *"is this line box tall enough for this face"* where it now
//! says *"is this run's ink taller than its box"*, and those are two questions.
//! The second is genuinely font-dependent and the paragraph is right about it.
//! The FIRST is not: [`crate::containment::line_box`] is a **reservation**
//! computed from the face size the author chose, with no face, no shaper and no
//! host in it — measured against the floor toolkit's own metrics it sits two to
//! three pixels ABOVE what any of them needs, deliberately. So it can be
//! decided here, at boot, in a sync `view`, and identically on every machine.
//!
//! Under the old wording the whole vertical axis read as un-gateable, and a
//! reader reported a clipped descender **twice, eleven days apart**, while this
//! tree held every number required to answer them. A design note that answers a
//! near-miss question closes the real one just as effectively as a wrong
//! implementation, and is harder to see.
//!
//! [`assert_boxes_hold_their_text`] is the check that paragraph was blocking.
//!
//! ## What the stand-in can and cannot say about height
//!
//! [`stand_in_ink`] answers a run's ink height with **the box's own height**.
//! That is right for what it is for — it feeds
//! [`escapes`](crate::containment::escapes), which asks whether a mark left its
//! PARENT — and it means the stand-in can never report a run overflowing its
//! own box downward. A reader must not take that silence for a clean bill:
//! `over_h == 0` from this metric is a structural constant, not a measurement.
//! `the_stand_in_cannot_answer_whether_a_box_is_too_short` pins it.

use crate::containment::{Escape, escapes};
use crate::scene::{Scene, TextNode};

/// The ink a screen gate measures a run with: a monospace stand-in for the
/// width, the laid-out line box for the height.
///
/// One function, because two stand-ins drift and a mark that is *inside its
/// box* under one measure and *out of reach* under another is a disagreement
/// about the screen rather than about the two questions.
#[must_use]
pub fn stand_in_ink(text: &TextNode) -> (u32, u32) {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a label is a handful of characters"
    )]
    let chars = text.content.chars().count() as u32;
    let px = text.style.font_size_px.max(1);
    // ★★★★★ R1797 — `bounds_ink`, not `shortens`. The two are different
    // questions and this asked the wrong one: `shortens` is about the CONTENT
    // (what introspection reports), and `Clip` is deliberately not one of its
    // arms because a clipped run still contains every character. What decides
    // how far the INK reaches is whether the arm confines the paint, and `Clip`
    // does — that is the whole of what it does. So a clipped label's hidden
    // glyphs were being counted as ink outside its box: marks nobody could see,
    // reported as marks painted outside.
    let painted = if text.style.overflow.bounds_ink() {
        text.rect.w.min(chars * px)
    } else {
        chars * px
    };
    (painted, text.rect.h)
}

/// Every mark that left the box that owns it, and how many of those were
/// entirely off-window.
///
/// The split is the grant, and it is *invisible*, not *low*: a mark whose
/// origin is past the window's right or bottom edge cannot be seen by anybody,
/// so it belongs to whatever registers the screen's scroll gap. A mark that is
/// partly visible **and** escaping is still a defect.
#[must_use]
pub fn ink_escapes(scene: &Scene, size: (u32, u32)) -> (Vec<Escape>, usize) {
    let found = escapes(scene, &mut stand_in_ink);
    let (offscreen, escaped): (Vec<_>, Vec<_>) = found
        .into_iter()
        .partition(|e| e.painted.y >= size.1 || e.painted.x >= size.0);
    (escaped, offscreen.len())
}

/// Assert that nothing left its box, naming what did; answers how many escapes
/// were off-window.
///
/// `when` is the state and size the sweep is in, so a failure says *which* of a
/// screen's states painted outside itself rather than only that one did.
///
/// # Panics
///
/// When any mark is outside the box that owns it and is not entirely
/// off-window.
pub fn assert_contained_ink(when: &str, scene: &Scene, size: (u32, u32)) -> usize {
    let (escaped, offscreen) = ink_escapes(scene, size);
    assert!(
        escaped.is_empty(),
        "{when}: {} painted mark(s) are outside the box that owns them — {:?}",
        escaped.len(),
        escaped
            .iter()
            .map(|e| (
                e.content.clone().or_else(|| e.tag.clone()),
                e.owner.clone(),
                e.over
            ))
            .take(6)
            .collect::<Vec<_>>()
    );
    offscreen
}

/// How many text runs the scene paints at all — the denominator every count of
/// short boxes needs to mean anything.
#[must_use]
pub fn runs_in(scene: &Scene) -> usize {
    let mut n = 0;
    scene.for_each_node(&mut |visit| {
        if matches!(visit.node, Scene::Text(_)) {
            n += 1;
        }
    });
    n
}

/// Assert that no run's own box was authored too short to hold it, naming the
/// worst offenders; answers how many were short.
///
/// # Why a budget rather than zero
///
/// `budget` is a **ratchet**: the count may fall or hold and may not rise. The
/// population was measured before this was written and it is not zero, and a
/// gate that demands zero on a tree that cannot yet give it is a gate somebody
/// turns off. R1656/R1664 established the idiom here for exactly that reason.
///
/// Pass `0` for a screen that has reached it. That is the direction of travel,
/// and a screen that reaches zero and then regresses fails on the next run.
///
/// # Panics
///
/// When more runs are short than the budget allows.
pub fn assert_boxes_hold_their_text(when: &str, scene: &Scene, budget: usize) -> usize {
    let short = crate::containment::short_boxes(scene);
    // ★ R1800 closing audit — the DENOMINATOR, because a count without its
    // population is not a measurement, and this round wrote "289 of 289" into
    // four files before noticing it had only ever measured the numerator. The
    // difference matters more than it looks: 289 short of 289 is a convention,
    // and 289 short of 2,890 is a backlog, and they call for opposite repairs.
    let total = runs_in(scene);
    assert!(
        short.len() <= budget,
        "{when}: {} of {total} run(s) are in a box too short for their own face, \
         budget {budget} — {:?}",
        short.len(),
        short
            .iter()
            .map(|s| (
                s.content.clone(),
                s.px,
                s.rect.h,
                s.needs,
                s.short_by,
                s.tag.clone()
            ))
            .take(6)
            .collect::<Vec<_>>()
    );
    short.len()
}

#[cfg(test)]
mod tests {
    use super::{assert_boxes_hold_their_text, assert_contained_ink, ink_escapes, stand_in_ink};
    use crate::containment::{line_box, short_boxes, short_by};
    use crate::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
    use crate::style::{BoxStyle, Color, TextStyle};

    /// ★★★★★ R1800 — the stand-in answers a run's ink HEIGHT with the box's own
    /// height, so `over_h` computed from it is a structural zero.
    ///
    /// Pinned rather than described, because "this metric cannot see that" is
    /// exactly the kind of sentence that stops being true quietly, and because
    /// a reader who finds `over_h == 0` here must be able to learn that it was
    /// never a measurement. The authoring rule is what does see it, and the
    /// second half asserts the two disagree on the same node — which is the
    /// whole reason both exist.
    #[test]
    fn the_stand_in_cannot_answer_whether_a_box_is_too_short() {
        let px = 12;
        let too_short = TextNode::styled(
            "packet gjpqy".to_owned(),
            Rect::new(0, 0, 120, line_box(px) - 6),
            TextStyle::default().with_size_px(px),
        );
        assert_eq!(
            stand_in_ink(&too_short).1,
            too_short.rect.h,
            "the stand-in answers the box's own height, whatever the face"
        );
        assert_eq!(
            stand_in_ink(&too_short).1.saturating_sub(too_short.rect.h),
            0,
            "so an over_h derived from it is zero by construction"
        );
        assert_eq!(
            short_by(&too_short),
            6,
            "while the authoring rule says the box is six pixels short"
        );
    }

    /// The predicate itself, at its edges.
    #[test]
    fn r1800_a_box_is_short_only_when_it_could_not_have_held_the_line() {
        let px = 12;
        let at = |h: u32| {
            short_by(&TextNode::styled(
                "config".to_owned(),
                Rect::new(0, 0, 60, h),
                TextStyle::default().with_size_px(px),
            ))
        };
        let need = line_box(px);
        assert_eq!(at(need), 0, "exactly enough is enough");
        assert_eq!(at(need + 40), 0, "and generous is fine");
        assert_eq!(at(need - 1), 1, "one short is one");
        assert_eq!(at(0), need, "a zero-height box is short by the whole line");
    }

    /// A wrapped run needs a line box PER LINE, and an unmeasured one is judged
    /// against one — the floor of the demand, not a guess at it.
    #[test]
    fn r1800_a_wrapped_run_needs_a_line_box_for_every_line() {
        let px = 12;
        let need = line_box(px);
        let mut node = TextNode::styled(
            "two lines of it".to_owned(),
            Rect::new(0, 0, 60, need),
            TextStyle::default().with_size_px(px),
        );
        assert_eq!(node.line_count, 0, "no shape pass has run");
        assert_eq!(short_by(&node), 0, "so one line is demanded, and it fits");
        node.line_count = 2;
        assert_eq!(
            short_by(&node),
            need,
            "two lines in a one-line box is short by a whole line"
        );
    }

    /// ★★★★★ The constructors cannot build a box the predicate then rejects.
    ///
    /// This is the property that makes the rule keepable: 289 of 289 runs on
    /// one screen failed it, which means telling authors the rule does not
    /// work — the rule has to be easier to obey than to break. Asserted across
    /// the face sizes this tree actually ships and a couple beyond them, so a
    /// change to `line_box` that broke the pairing lands here.
    #[test]
    fn r1800_a_box_built_by_the_rule_cannot_fail_the_rule() {
        use crate::containment::{line_rect, line_rect_in};
        for px in [1_u32, 8, 10, 11, 12, 13, 14, 16, 24, 48] {
            let built = line_rect(16, 19, 96, px);
            let node = TextNode::styled(
                "packet gjpqy".to_owned(),
                built,
                TextStyle::default().with_size_px(px),
            );
            assert_eq!(short_by(&node), 0, "line_rect at px={px} is short");

            let bar = Rect::new(0, 0, 400, 54);
            let centred = line_rect_in(bar, 16, 96, px);
            let node = TextNode::styled(
                "packet gjpqy".to_owned(),
                centred,
                TextStyle::default().with_size_px(px),
            );
            assert_eq!(short_by(&node), 0, "line_rect_in at px={px} is short");
            // Centred means the slack is shared, within the odd pixel.
            if centred.h <= bar.h {
                let above = centred.y - bar.y;
                let below = bar.h - (above + centred.h);
                assert!(
                    above.abs_diff(below) <= 1,
                    "px={px}: {above} above, {below} below"
                );
            }
        }
    }

    /// The census reports the amount per run and skips what makes no promise.
    #[test]
    fn r1800_the_census_names_the_short_runs_and_only_those() {
        let px = 12;
        let need = line_box(px);
        let scene = Scene::Container(ContainerNode {
            rect: Rect::new(0, 0, 300, 200),
            children: vec![
                run("tall enough", Rect::new(0, 0, 100, need), px),
                run("three short", Rect::new(0, 40, 100, need - 3), px),
                // No extent: it promises to hold nothing, so it cannot fail.
                run("no box", Rect::new(0, 80, 0, 0), px),
            ],
            ..ContainerNode::default()
        });
        let found = short_boxes(&scene);
        assert_eq!(found.len(), 1, "one short run, not three: {found:?}");
        assert_eq!(found[0].content, "three short");
        assert_eq!(found[0].short_by, 3);
        assert_eq!(found[0].needs, need);
        assert_eq!(found[0].px, px);
        // And the assert wrapper agrees with the census it wraps.
        assert_eq!(assert_boxes_hold_their_text("probe", &scene, 1), 1);
    }

    fn run(content: &str, rect: Rect, px: u32) -> Scene {
        Scene::Text(TextNode::styled(
            content.to_owned(),
            rect,
            TextStyle::default().with_size_px(px),
        ))
    }

    /// ★★★★★ R1797 — a CLIPPED run's hidden glyphs are not ink that escaped.
    ///
    /// The stand-in asked `shortens()`, which is about the CONTENT — and `Clip`
    /// is deliberately not one of its arms, because a clipped run still holds
    /// every character. What decides how far the ink reaches is whether the arm
    /// confines the paint. So a run far too long for its box, clipped, was
    /// reported as painting outside it, and every one of those glyphs is
    /// scissored away before anything reaches a screen.
    ///
    /// Both directions, because the repair must not have made the stand-in
    /// answer "it fits" to everything — that is the failure the negative
    /// control below exists for.
    #[test]
    fn r1797_a_clipped_run_paints_no_ink_past_its_own_box() {
        use crate::style::TextOverflow;
        let long = "a string far longer than the box it was given";
        let boxed = |overflow: TextOverflow| {
            stand_in_ink(&TextNode::styled(
                long.to_owned(),
                Rect::new(0, 0, 40, 14),
                TextStyle::default()
                    .with_size_px(12)
                    .with_overflow(overflow),
            ))
            .0
        };
        assert_eq!(
            boxed(TextOverflow::Clip),
            40,
            "clipped ink stops at the box"
        );
        assert_eq!(boxed(TextOverflow::Ellipsis), 40, "and so does an ellipsis");
        assert!(
            boxed(TextOverflow::Visible) > 40,
            "while a run that paints past its edge still measures past it: {}",
            boxed(TextOverflow::Visible)
        );
        for arm in TextOverflow::ALL {
            assert_eq!(
                arm.bounds_ink(),
                arm != TextOverflow::Visible,
                "{arm:?} — every arm but Visible confines the paint"
            );
        }
    }

    /// ★ R1672 — the gate MEASURES: a scene built to break it is reported.
    ///
    /// A negative control, and the reason it is in the crate rather than in the
    /// screens: a helper whose stand-in always answered "it fits" would report
    /// every screen as clean, and a check that cannot fail is indistinguishable
    /// from a screen with nothing wrong ([[r1644-1]]). This is the assertion
    /// that says the vocabulary is load-bearing before any screen borrows it.
    #[test]
    fn r1672_the_ink_gate_reports_a_scene_built_to_break_it() {
        let mut card = ContainerNode::new(vec![run(
            "a string far longer than the box it was given",
            Rect::new(0, 0, 200, 14),
            12,
        )]);
        card.rect = Rect::new(0, 0, 40, 20);
        card.tag = Some("card".to_owned().into());
        let scene = Scene::Container(card);

        let (escaped, offscreen) = ink_escapes(&scene, (400, 400));
        assert_eq!(offscreen, 0, "the mark is on screen, so it is a defect");
        assert_eq!(escaped.len(), 1, "the run left its card: {escaped:?}");
        assert_eq!(escaped[0].owner, "card");
        assert!(escaped[0].over.right > 0, "past the right edge");
    }

    /// ★ R1672 — a mark whose origin is past the window is COUNTED, not
    /// asserted.
    ///
    /// The counter-half of the test above: without it, "reports an escape"
    /// could be satisfied by a gate that reports everything, and the off-window
    /// grant is exactly the case a screen's registered scroll gap depends on.
    #[test]
    fn r1672_a_mark_entirely_off_window_is_counted_not_asserted() {
        let mut card = ContainerNode::new(vec![run(
            "a string far longer than the box it was given",
            Rect::new(0, 500, 200, 14),
            12,
        )]);
        card.rect = Rect::new(0, 500, 40, 20);
        card.tag = Some("card".to_owned().into());
        let scene = Scene::Container(card);

        let below = assert_contained_ink("below the window", &scene, (400, 400));
        assert_eq!(below, 1, "counted as off-window rather than asserted");
    }

    /// ★ R1672 — a mark that fits is not reported, so the gate is not simply
    /// always red.
    #[test]
    fn r1672_a_mark_inside_its_box_is_not_reported() {
        let mut card = ContainerNode::new(vec![Scene::Box(BoxNode::new(
            Rect::new(2, 2, 20, 10),
            BoxStyle::filled(Color::rgb(0x20, 0x20, 0x20)),
        ))]);
        card.rect = Rect::new(0, 0, 40, 20);
        card.tag = Some("card".to_owned().into());
        let (escaped, offscreen) = ink_escapes(&Scene::Container(card), (400, 400));
        assert!(escaped.is_empty(), "it fits: {escaped:?}");
        assert_eq!(offscreen, 0);
    }

    /// ★ R1672 — the stand-in's width does not depend on the host's fonts, and
    /// it SHORTENS when the run's overflow policy says the paint will.
    #[test]
    fn r1672_the_stand_in_width_follows_the_overflow_policy() {
        use crate::style::TextOverflow;
        let long = "0123456789abcdef";
        let boxed = Rect::new(0, 0, 30, 14);
        let clipped = TextNode::styled(
            long.to_owned(),
            boxed,
            TextStyle::default()
                .with_size_px(10)
                .with_overflow(TextOverflow::Ellipsis),
        );
        let spilling = TextNode::styled(
            long.to_owned(),
            boxed,
            TextStyle::default()
                .with_size_px(10)
                .with_overflow(TextOverflow::Visible),
        );
        assert_eq!(
            stand_in_ink(&clipped).0,
            30,
            "a run the paint will shorten is measured at its box"
        );
        assert_eq!(
            stand_in_ink(&spilling).0,
            160,
            "a run the paint will not shorten is measured at its whole string"
        );
        assert_eq!(
            stand_in_ink(&spilling).1,
            14,
            "the HEIGHT is the laid-out line box, not a made-up number"
        );
    }
}
