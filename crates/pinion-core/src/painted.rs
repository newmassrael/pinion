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

use crate::scene::{Rect, Scene};

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
/// ★★★★★ R1770 — **the size of the surface a frame was painted into.**
///
/// # Why a verdict that does not carry this is incomplete
///
/// Measured at R1767 on this tree's own analysis tool, walking the same binary
/// twice and moving one variable — the window: at 1440x900 two of the node
/// lab's surfaces do not reproduce their specification, and at 2494x1531 those
/// two reconcile and a **different** section's does not. Both walks reported
/// `conforms=false`, the two failing sets are disjoint, and nothing in either
/// report said which window it was read at. So the two readings could not be
/// told apart by anything except a person remembering which run was which.
///
/// It is the same rule R1752 wrote for a duration (*a number that says how many
/// must say what of*) and R1758 wrote for evidence (*a verdict says what it was
/// read from*), one turn further: **a verdict says what size it was read at**.
///
/// # Why the SURFACE and not the window
///
/// A section mounted as a page of an assembled tool is given a fraction of the
/// window — measured R1761, 1096x802 of a 1440x900 window — and what it can
/// draw is a fact about that fraction, not about the window. The store records
/// each surface's own rectangle already, so this is the size the marks below
/// are in the coordinates of, and no caller has to relate two frames of
/// reference to use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Extent {
    width: u32,
    height: u32,
}

impl Extent {
    /// The extent `width` by `height`.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// The extent of a rectangle — its size, with its position discarded.
    #[must_use]
    pub const fn of(rect: Rect) -> Self {
        Self::new(rect.w, rect.h)
    }

    /// How wide.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// How tall.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

impl core::fmt::Display for Extent {
    /// `1096x802` — the spelling a specification pin and a wire report both
    /// use, so a size read off one can be searched for in the other.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaintedRegions {
    /// Tag and rectangle, in the order the frame drew them: earliest first.
    marks: Vec<(String, Rect)>,
    /// ★ R1742 — tag -> what the text runs it owns read, joined in paint order.
    ///
    /// See [`reads`](Self::reads) for what this is for and what it is not.
    reads: BTreeMap<String, String>,
    /// ★ R1770 — the size of the surface these marks were painted into.
    ///
    /// `None` for a caller that built the store by hand out of marks alone and
    /// therefore knows of no frame. See [`extent`](Self::extent) for why that
    /// absence is not the same as a size of zero, and [`Extent`] for what a
    /// verdict read without one cannot say.
    extent: Option<Extent>,
}

impl PaintedRegions {
    /// Build from marks already in paint order, earliest first.
    ///
    /// The frame's words are not recorded by this constructor — see
    /// [`with_reads`](Self::with_reads). Kept as the plain constructor because
    /// most callers of this store are asking *where*, and a caller that has no
    /// words to record should not have to say so with an empty map.
    #[must_use]
    pub fn from_marks(marks: Vec<(String, Rect)>) -> Self {
        Self {
            marks,
            reads: BTreeMap::new(),
            extent: None,
        }
    }

    /// ★ R1742 — the same marks, with what each tag's runs read.
    #[must_use]
    pub fn with_reads(mut self, reads: BTreeMap<String, String>) -> Self {
        self.reads = reads;
        self
    }

    /// ★ R1770 — the same marks, with the size of the surface they were
    /// painted into.
    ///
    /// Separate from the constructor for the reason
    /// [`with_reads`](Self::with_reads) is: most readers of this store are
    /// asking *where*, and a caller with no frame to speak of should not have
    /// to invent a size to say so.
    #[must_use]
    pub fn with_extent(mut self, extent: Extent) -> Self {
        self.extent = Some(extent);
        self
    }

    /// ★★★★★ R1770 — the size of the surface this frame was painted into, or
    /// `None` for a store built without one.
    ///
    /// # Why `None` is not `0x0`
    ///
    /// A verdict read from a store with no extent cannot say what size it is
    /// about, and a conformance ledger whose entries are size-dependent
    /// **refuses** such a verdict rather than judging it — see that module's
    /// unreconciled arm for a verdict of unknown size. Collapsing the two would
    /// make the most flattering reading of an unknown size the default, which
    /// is the failure mode every refusal in that module exists to prevent.
    #[must_use]
    pub const fn extent(&self) -> Option<Extent> {
        self.extent
    }

    /// ★★★★★ R1742 — **what `tag` reads**: the text the frame drew inside it,
    /// in paint order, or `None` when it drew none.
    ///
    /// # Why the store holds this and not only rectangles
    ///
    /// The module's own sentence is *what a surface's last frame actually
    /// painted, readable from anywhere*, and until this round it meant only
    /// **where**. A caller asking what a surface says had two routes and both
    /// are worse: walk a scene it does not have, or read a screenshot — which
    /// is the pixels-carry-no-identity floor this module exists to be past.
    ///
    /// It is what lets a screen judge its own painted words. The node lab's
    /// enumeration roster is specified by the words it draws, so a roster whose
    /// third row drew the second word is a defect, and a check that read the
    /// words out of the model instead would be a second account of the same
    /// fact and would agree with the model by construction.
    ///
    /// ⚠ **The runs' content, not the glyphs.** A label the frame elided to
    /// `"Conn…"` reads here as what it holds; what the shaper actually fitted is
    /// `pinion_rpc`'s painted-text report, which needs the shape cache this
    /// store does not have. Stated rather than left for a reader to discover:
    /// this answers *what was given to be drawn under this name*.
    ///
    /// Several runs under one tag are joined with a single space, in paint
    /// order, because that is the order a reader reads them in.
    #[must_use]
    pub fn reads(&self, tag: &str) -> Option<&str> {
        self.reads.get(tag).map(String::as_str)
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

    /// ★ R1742 — every tagged rectangle a scene paints, in paint order and in
    /// the scene's own coordinates, with what each name reads.
    ///
    /// The entry point for a caller that *has* a scene rather than a recorded
    /// surface — the in-process paint fixtures. It exists so the readings below
    /// are one rule with two entry points instead of one rule written twice:
    /// before this, a screen judged from a scene walk and the running window
    /// resolved presses from the recorded store, which is the two-derivations
    /// shape this whole module exists to remove.
    #[must_use]
    pub fn of_scene(scene: &Scene) -> Self {
        let mut marks: Vec<(String, Rect)> = Vec::new();
        let mut reads: BTreeMap<String, String> = BTreeMap::new();
        let mut extent: Option<Extent> = None;
        scene.for_each_node(&mut |visit| {
            let Some(rect) = visit.absolute_rect() else {
                return;
            };
            if let Scene::Text(text) = visit.node {
                let owner = text
                    .tag
                    .as_deref()
                    .or_else(|| visit.ancestors.iter().rev().find_map(|a| a.tag()));
                if let Some(owner) = owner {
                    reads
                        .entry(owner.to_owned())
                        .and_modify(|held| {
                            held.push(' ');
                            held.push_str(&text.content);
                        })
                        .or_insert_with(|| text.content.clone());
                }
            }
            if visit.ancestors.is_empty() {
                // ★ R1770 — the root's own rectangle IS the extent this walk is
                // in the coordinates of. Taken here rather than from the caller
                // so a fixture cannot hand a size the scene does not have,
                // which is the same one-directional rule the recorded path gets
                // for free from the surface rectangle it already holds.
                extent = Some(Extent::of(rect));
            }
            if let Some(tag) = visit.node.tag() {
                marks.push((tag.to_owned(), rect));
            }
        });
        Self {
            marks,
            reads,
            extent,
        }
    }

    /// ★ R1742 — the parts under `stem` whose remainder names a part, in paint
    /// order.
    ///
    /// **Which tags belong to a surface**: the ones whose remainder after
    /// `stem` holds no further dot. A derivation rather than a list of
    /// exclusions — a part's own decoration is tagged *inside* it, so it is
    /// excluded by the shape of its name rather than by being remembered, and a
    /// gate that named the chrome it had to skip was only ever as good as
    /// whoever last updated the list (R1728 is that case).
    ///
    /// The first painted occurrence of a key wins, because a surface's parts
    /// are its own and a decoration redrawing the same name later is not a
    /// second part.
    #[must_use]
    pub fn parts_under(&self, stem: &str) -> Vec<(String, Rect)> {
        let mut found: Vec<(String, Rect)> = Vec::new();
        for (tag, rect) in self.marks() {
            let Some(key) = tag.strip_prefix(stem) else {
                continue;
            };
            if key.is_empty() || key.contains('.') || found.iter().any(|(seen, _)| seen == key) {
                continue;
            }
            found.push((key.to_owned(), rect));
        }
        found
    }

    /// ★ R1742 — the parts of one `address`, for a surface tagged **family
    /// first**: `<prefix><part>.<address>`. In reading order.
    ///
    /// [`parts_under`](Self::parts_under) reads the other convention — an
    /// address, then the part under it — and takes the remainder holding no dot
    /// as the part's name. A form row is tagged the other way round, because
    /// the thing being addressed is a configuration path and every one of them
    /// contains dots: no rule about counting dots can find the seam, so the
    /// seam is given rather than guessed.
    #[must_use]
    pub fn parts_of(&self, prefix: &str, address: &str) -> Vec<(String, Rect)> {
        let tail = format!(".{address}");
        let mut found: Vec<(String, Rect)> = Vec::new();
        for (tag, rect) in self.marks() {
            let Some(key) = tag
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(tail.as_str()))
            else {
                continue;
            };
            if key.is_empty() || found.iter().any(|(seen, _)| seen == key) {
                continue;
            }
            found.push((key.to_owned(), rect));
        }
        in_reading_order(found)
    }
}

/// ★★★★★ R1742 — painted parts in **reading order**: parts whose rectangles
/// overlap vertically are on one line and sort across it; lines sort down.
///
/// One rule for every kind of surface — a row of parts, a column of them, and a
/// two-by-two grid of single facts, which is two parts on one line twice.
///
/// ★ The naive `(y, x)` sort was wrong and R1730's gate said so on its first
/// run: a section header's three parts were vertically CENTRED against
/// different heights, so the filter box's top edge sat seven pixels above the
/// title's and it sorted first. Aligning the boxes to make the sort work would
/// have been fixing the screen to suit the check.
///
/// Lives here rather than beside the paint fixtures because it is a rule about
/// what a reader sees, and the running application now judges itself by it.
#[must_use]
pub fn in_reading_order(mut parts: Vec<(String, Rect)>) -> Vec<(String, Rect)> {
    parts.sort_by_key(|(key, rect)| (rect.y, rect.x, key.clone()));
    let mut ordered: Vec<(String, Rect)> = Vec::with_capacity(parts.len());
    let mut line: Vec<(String, Rect)> = Vec::new();
    let mut bottom = 0;
    for (key, rect) in parts {
        if !line.is_empty() && rect.y >= bottom {
            line.sort_by_key(|(_, r)| r.x);
            ordered.append(&mut line);
            bottom = 0;
        }
        bottom = bottom.max(rect.y + rect.h);
        line.push((key, rect));
    }
    line.sort_by_key(|(_, r)| r.x);
    ordered.append(&mut line);
    ordered
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

    /// ★ R1742 — the store answers what a tag READS, not only where it is.
    #[test]
    fn r1742_a_mark_says_what_was_drawn_inside_it() {
        let r = regions().with_reads(
            [
                ("card".to_owned(), "peer_to_peer".to_owned()),
                ("pin".to_owned(), "in out".to_owned()),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(r.reads("card"), Some("peer_to_peer"));
        // Several runs under one name read in paint order, joined — because
        // that is the order a reader reads them in.
        assert_eq!(r.reads("pin"), Some("in out"));
        // ★ A mark that drew no words says so, rather than saying it drew none:
        // `Some("")` and `None` are different facts and a caller comparing a
        // specified word with what was drawn needs the second one.
        assert_eq!(r.reads("canvas"), None);
        assert_eq!(r.reads("nothing-of-the-kind"), None);
        // And the rectangles are untouched by carrying words.
        assert_eq!(r.topmost_at(22, 26), Some("pin"));
    }

    /// ★★★★★ R1742 — reading a scene fills BOTH halves: where each name was
    /// painted, and what it reads.
    ///
    /// The ancestor case is the one worth a fixture: a run carrying no tag of
    /// its own belongs to the nearest tagged thing above it, which is the shape
    /// almost every label in this tree has — a box with a name and a run inside
    /// it that has none.
    #[test]
    fn r1742_reading_a_scene_gives_the_marks_and_the_words() {
        use crate::scene::{ContainerNode, Scene, TextNode};
        use crate::style::TextStyle;

        let run = |text: &str, rect: Rect| {
            Scene::Text(TextNode::styled(
                text.to_owned(),
                rect,
                TextStyle::default(),
            ))
        };
        let scene = Scene::Container(ContainerNode::new(vec![
            // ★★★★★ A named box INSIDE another named box, so "the nearest
            // tagged ancestor" and "the outermost one" are different answers.
            //
            // Written the second time, and the first version is the lesson: it
            // put the named box directly under an untagged root, where the two
            // coincide — and a counterfactual that walked the ancestors the
            // wrong way round PASSED. A fixture that cannot tell two
            // expressions apart is the same as having no gate.
            Scene::Container(
                ContainerNode::new(vec![Scene::Container(
                    ContainerNode::new(vec![
                        run("peer_to_peer", Rect::new(10, 10, 60, 12)),
                        run("mode", Rect::new(10, 24, 30, 12)),
                    ])
                    .with_tag("option".to_owned()),
                )])
                .with_tag("panel".to_owned()),
            ),
            // And a run that carries its own name.
            Scene::Text(
                TextNode::styled(
                    "router".to_owned(),
                    Rect::new(80, 10, 40, 12),
                    TextStyle::default(),
                )
                .with_tag("shown".to_owned()),
            ),
        ]));

        let read = PaintedRegions::of_scene(&scene);
        assert_eq!(
            read.reads("option"),
            Some("peer_to_peer mode"),
            "★ two runs under one name read in paint order, joined",
        );
        assert_eq!(
            read.reads("panel"),
            None,
            "★★★★★ and the box FURTHER OUT reads nothing -- a run belongs to the \
             nearest name above it, and this is the assertion that can tell the \
             two walks apart",
        );
        assert_eq!(read.reads("shown"), Some("router"));
        assert_eq!(read.reads("nothing-of-the-kind"), None);
        // The mark half is unchanged: a laid-out run carries its rectangle.
        assert_eq!(read.rect_of("shown"), Some(Rect::new(80, 10, 40, 12)));
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
