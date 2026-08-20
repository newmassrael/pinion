//! ★★★★★ R1736 §5.35 §5.15 §2 #7 — **what a surface's last frame actually
//! painted, readable from anywhere.**
//!
//! # The hole this closes
//!
//! §2 #7 makes a pinion screen ONE [`External`](crate::external::External), so
//! the router resolves a press to the screen and the screen resolves it the
//! rest of the way. Every such screen therefore owns a hit test, and every one
//! of them written so far computes its rectangles **a second time**: the view
//! function lays a card out and the press path works out where that card must
//! have ended up. One fact, two derivations, free to part.
//!
//! They have parted, repeatedly, and always in the same shape — a transform
//! applied on one side and not the other. R1648 drew cards at double their
//! offset. R1651.1 drew rail seats in window coordinates and resolved presses
//! in pane coordinates. R1662 scrolled the palette's paint and not its hit
//! test. R1700 reflowed a screen to a new window size while its hit test went
//! on answering for the old one, and 166 rectangles stopped being pressable
//! where they were drawn.
//!
//! Each was repaired by hand, on one screen, and each repair left the structure
//! that produced it in place. This module removes the structure: the framework
//! painted those rectangles and still holds them, so a screen can **ask** where
//! it drew something instead of working it out again.
//!
//! # What is stored
//!
//! Every tagged rectangle of the surface's last painted frame, **in paint
//! order** and in the **surface's own coordinates** — the frame
//! [`External::target_at`] is asked in and the frame
//! [`layout_point`](crate::external::layout_point) answers in.
//!
//! Paint order is the load-bearing half. A hit test's whole job is to decide
//! which of several overlapping things a point belongs to, and a screen that
//! hand-orders that decision is keeping a second z-order beside the painter's —
//! the same class one level up. Here the answer is the paint's own: last drawn
//! is topmost, which is what the reader sees.
//!
//! The rectangles are the CLIPPED ones ([`NodeVisit::absolute_rect`]), so a
//! mark scrolled half out of a pane is pressable over exactly the half that is
//! showing. "Reported" and "visible" stay one fact.
//!
//! [`External::target_at`]: crate::external::External::target_at
//! [`NodeVisit::absolute_rect`]: crate::NodeVisit::absolute_rect
//!
//! # Where the floor stands, measured
//!
//! Built as a probe against the 6.11.1 release and run, rather than read out of
//! its documentation.
//!
//! A canvas item that paints exactly the region it declares is pickable at
//! **100% of the pixels it was drawn in**, under four camera transforms
//! including fractional zooms — 13,500, 9,450, 11,718 and 9,919 points, no
//! misses. That is the floor for this property and it is a real one.
//!
//! What the floor cannot do is keep the two in step when an author lets them
//! part, or notice that they have. An item there declares where it may draw and
//! declares, separately, where a press lands. The same probe measured an item
//! whose paint reaches six pixels past its pick shape: **15.4%** of its painted
//! pixels at zoom 100 and 84, and **30.1%** at 135, resolve to nothing at all.
//! The framework holds both rectangles, hands out the larger one as the item's
//! bounds, and never compares them — there is no verdict to read and nothing to
//! ask. And for a plain self-painting widget the framework holds no record of
//! the paint at all: of eight painted marks it names **0**, because the only
//! thing it kept is pixels, which carry no identity.
//!
//! Introspection-from-paint is what makes this module possible. A screen here
//! can resolve a press against what it drew because the framework kept what it
//! drew, with the names still on it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::scene::Rect;

thread_local! {
    /// Surface tag -> what that surface's last painted frame drew.
    static PAINTED: RefCell<BTreeMap<String, Rc<PaintedRegions>>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// The tagged rectangles of one surface's last painted frame, in paint order.
///
/// ```
/// # use pinion_core::painted::PaintedRegions;
/// # use pinion_core::scene::Rect;
/// let regions = PaintedRegions::from_marks(vec![
///     ("board".to_owned(), Rect::new(0, 0, 100, 100)),
///     ("card".to_owned(), Rect::new(10, 10, 40, 40)),
/// ]);
/// // Last drawn wins, which is what the reader sees.
/// assert_eq!(regions.topmost_at(20, 20), Some("card"));
/// assert_eq!(regions.topmost_at(80, 80), Some("board"));
/// assert_eq!(regions.topmost_at(400, 400), None);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaintedRegions {
    /// Tag and rectangle, in the order the frame drew them: earliest first.
    marks: Vec<(String, Rect)>,
}

impl PaintedRegions {
    /// Build from marks already in paint order, earliest first.
    #[must_use]
    pub fn from_marks(marks: Vec<(String, Rect)>) -> Self {
        Self { marks }
    }

    /// Every tag painted over `(x, y)`, **topmost first**.
    ///
    /// The order is the whole point: a caller takes the first answer it
    /// recognises and is thereby agreeing with what the reader sees, rather
    /// than keeping a z-order of its own beside the painter's.
    pub fn stack_at(&self, x: u32, y: u32) -> impl Iterator<Item = (&str, Rect)> {
        self.marks
            .iter()
            .rev()
            .filter(move |(_, rect)| rect.holds(x, y))
            .map(|(tag, rect)| (tag.as_str(), *rect))
    }

    /// The topmost tag painted over `(x, y)`, or `None` where this surface drew
    /// nothing that carries a name.
    #[must_use]
    pub fn topmost_at(&self, x: u32, y: u32) -> Option<&str> {
        self.stack_at(x, y).next().map(|(tag, _)| tag)
    }

    /// Where `tag` was painted, or `None` if this frame did not draw it.
    ///
    /// The **topmost** occurrence, so a tag drawn twice answers with the one a
    /// press would reach.
    #[must_use]
    pub fn rect_of(&self, tag: &str) -> Option<Rect> {
        self.marks
            .iter()
            .rev()
            .find(|(name, _)| name == tag)
            .map(|(_, rect)| *rect)
    }

    /// Every mark, in paint order.
    pub fn marks(&self) -> impl Iterator<Item = (&str, Rect)> {
        self.marks.iter().map(|(tag, rect)| (tag.as_str(), *rect))
    }

    /// How many marks this frame drew.
    #[must_use]
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Whether this frame drew nothing that carries a name.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }
}

/// ★★★★★ R1736 — **the nine points a painted rectangle is judged at**: its
/// centre first, then the four edge midpoints, then the four corners, each
/// inset far enough to be unambiguously inside.
///
/// # Why the framework owns the sampling rule
///
/// Two things ask it — the framework's own `scene/pointer_target` census and a
/// screen's paint sweep — and they are asking the SAME question about the same
/// rectangles. Two copies of "the whole rectangle" would be two definitions of
/// what this workspace's gate means, and the screen that disagreed would be the
/// one nobody re-derived.
///
/// # Why nine and not one, and why not every pixel
///
/// One was what both of them used until a person found the defect the middle
/// cannot see: every automatic observer in this tree aims at the centre, so the
/// centre is the last point a transform error moves and the last point to
/// convict. These nine are the ones it moves first.
///
/// Every pixel is the honest question and is what a per-control probe can
/// afford; nine is what a check can afford to ask of every painted rectangle of
/// every screen. The gap is written down rather than glossed —
/// `docs/analyzer-press-spec.json` carries it as an owed clause.
///
/// The centre is index 0, so a caller that still wants the strong form alone
/// can take it without knowing the order.
///
/// ```
/// # use pinion_core::painted::probe_points;
/// # use pinion_core::scene::Rect;
/// let r = Rect::new(100, 50, 40, 20);
/// let points = probe_points(r);
/// assert_eq!(points[0], (120, 60), "the centre comes first");
/// assert!(points.iter().all(|(x, y)| r.holds(*x, *y)), "and all nine are inside");
/// ```
#[must_use]
pub const fn probe_points(rect: Rect) -> [(u32, u32); 9] {
    // `saturating_sub` throughout: this is asked about every painted rectangle
    // of every screen, including the degenerate ones, and it runs inside a
    // dispatcher where a panic takes the whole surface down.
    let inset_x = if rect.w / 4 < 6 { rect.w / 4 } else { 6 };
    let inset_y = if rect.h / 4 < 6 { rect.h / 4 } else { 6 };
    let left = rect.x + inset_x;
    let right = rect.x + rect.w.saturating_sub(1).saturating_sub(inset_x);
    let top = rect.y + inset_y;
    let bottom = rect.y + rect.h.saturating_sub(1).saturating_sub(inset_y);
    let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    [
        (cx, cy),
        (cx, top),
        (cx, bottom),
        (left, cy),
        (right, cy),
        (left, top),
        (right, top),
        (left, bottom),
        (right, bottom),
    ]
}

/// What the surface tagged `surface_tag` painted last frame, or `None` for a
/// surface that has not been painted.
///
/// `None` is the truthful answer for a screen asked before its first frame, and
/// a caller that has a model to fall back on must say so itself rather than
/// being handed an empty set that reads as "nothing is there" — the same
/// distinction [`surface_size`](crate::external::surface_size) draws.
#[must_use]
pub fn painted_regions(surface_tag: &str) -> Option<Rc<PaintedRegions>> {
    PAINTED.with(|painted| painted.borrow().get(surface_tag).cloned())
}

/// Record what a surface just painted — called by the layer that runs the
/// layout pass, never by a widget.
///
/// ★ Public because the recorder lives in `pinion-runtime`, one crate up, for
/// the reason [`record_surface_size`](crate::external::record_surface_size) is:
/// it is the framework's own bookkeeping. A widget calling this would be
/// declaring where it drew rather than being told.
pub fn record_painted_regions(surface_tag: &str, regions: PaintedRegions) {
    PAINTED.with(|painted| {
        painted
            .borrow_mut()
            .insert(surface_tag.to_owned(), Rc::new(regions));
    });
}

/// Forget a surface that is no longer painted, so a stale frame cannot answer
/// for something that is not on screen.
pub fn forget_painted_regions(surface_tag: &str) {
    PAINTED.with(|painted| {
        painted.borrow_mut().remove(surface_tag);
    });
}

#[cfg(test)]
mod tests {
    use super::{PaintedRegions, forget_painted_regions, painted_regions, record_painted_regions};
    use crate::scene::Rect;

    fn regions() -> PaintedRegions {
        PaintedRegions::from_marks(vec![
            ("canvas".to_owned(), Rect::new(0, 0, 200, 200)),
            ("card".to_owned(), Rect::new(20, 20, 60, 40)),
            ("pin".to_owned(), Rect::new(16, 24, 8, 8)),
        ])
    }

    #[test]
    fn r1736_the_last_thing_drawn_is_the_first_thing_answered() {
        let r = regions();
        // The pin overhangs the card's left edge and was drawn after it, so a
        // point in both belongs to the pin — which is what the reader sees.
        assert_eq!(r.topmost_at(22, 26), Some("pin"));
        // A point on the card and not the pin is the card's.
        assert_eq!(r.topmost_at(40, 40), Some("card"));
        // And one on neither falls through to what was drawn underneath.
        assert_eq!(r.topmost_at(150, 150), Some("canvas"));
    }

    #[test]
    fn r1736_the_stack_is_every_answer_not_only_the_first() {
        let r = regions();
        let stack: Vec<&str> = r.stack_at(22, 26).map(|(tag, _)| tag).collect();
        // Topmost first, all the way down — so a caller that does not
        // recognise the pin can go on to the card without inventing an order.
        assert_eq!(stack, ["pin", "card", "canvas"]);
    }

    #[test]
    fn r1736_a_rectangle_holds_its_own_edges_and_not_its_neighbours() {
        let r = PaintedRegions::from_marks(vec![("a".to_owned(), Rect::new(10, 10, 5, 5))]);
        // Half-open in both axes, the same rule every other rectangle test in
        // this tree uses — so two rectangles that touch do not both claim the
        // seam.
        assert_eq!(r.topmost_at(10, 10), Some("a"));
        assert_eq!(r.topmost_at(14, 14), Some("a"));
        assert_eq!(r.topmost_at(15, 14), None);
        assert_eq!(r.topmost_at(14, 15), None);
        assert_eq!(r.topmost_at(9, 10), None);
    }

    #[test]
    fn r1736_a_tag_drawn_twice_answers_with_the_one_a_press_reaches() {
        let r = PaintedRegions::from_marks(vec![
            ("row".to_owned(), Rect::new(0, 0, 10, 10)),
            ("row".to_owned(), Rect::new(50, 50, 10, 10)),
        ]);
        assert_eq!(r.rect_of("row"), Some(Rect::new(50, 50, 10, 10)));
    }

    #[test]
    fn r1736_an_unpainted_surface_answers_nothing_rather_than_empty() {
        // The `surface_size` distinction: a screen that has not painted is not
        // a screen that painted nothing, and a caller must be able to tell.
        forget_painted_regions("r1736.probe");
        assert!(painted_regions("r1736.probe").is_none());
        record_painted_regions("r1736.probe", PaintedRegions::default());
        let held = painted_regions("r1736.probe").expect("just recorded");
        assert!(held.is_empty());
        forget_painted_regions("r1736.probe");
        assert!(painted_regions("r1736.probe").is_none());
    }
}
