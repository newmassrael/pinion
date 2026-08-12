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
//! the question an author gets wrong. *"Is this line box tall enough for this
//! face"* is answered by the layout pass from the host's real metrics, and
//! re-deciding it here against a constant would make the gate green or red
//! depending on which fonts are installed ([[zero-flake-policy]]).
//!
//! The shaped answer to both is what `scene/containment` reports at boot, on
//! the machine that is actually painting.

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
    let painted = if text.style.overflow.shortens() {
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

#[cfg(test)]
mod tests {
    use super::{assert_contained_ink, ink_escapes, stand_in_ink};
    use crate::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
    use crate::style::{BoxStyle, Color, TextStyle};

    fn run(content: &str, rect: Rect, px: u32) -> Scene {
        Scene::Text(TextNode::styled(
            content.to_owned(),
            rect,
            TextStyle::default().with_size_px(px),
        ))
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
