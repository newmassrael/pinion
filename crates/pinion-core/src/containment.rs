//! R1656 §5.32 §5.36 §2 #7 — **did what the frame painted stay inside the box
//! it was promised?**
//!
//! # The half-fact this closes
//!
//! A rectangle in a scene is a *promise*: this mark will be drawn here, in this
//! much room. Every read in this tree reports the promise. Nothing reported
//! whether it was kept, so a screen could paint a label across the row below it
//! — or past the edge of the card that owns it — and describe itself as
//! correct to every gate, every test and every agent.
//!
//! That is not hypothetical. Measured on the analysis-tool screen at the size
//! it opens in: **seven of its eight node cards paint their last field row
//! outside the card**, three to five pixels below the border, and one link
//! label reaches eighty-two pixels past the right edge. A person saw it
//! immediately. The tree had four checks that could each have caught it and
//! none of them asked this question:
//!
//! * `scene/snapshot` reports the promised rectangles, which are all correct —
//!   the *ink* is what left the box, and the ink is not in the scene.
//! * `scene/text_painted`'s `overflows` compares a run's ink against **its own**
//!   box, which is a different question and answers `true` for 124 of 157 runs
//!   on that screen: an authored line box a few pixels shorter than the shaped
//!   line box is near-universal and benign, so the flag cannot discriminate.
//! * the smear gate groups runs by their nearest **tagged ancestor**, and a
//!   painter that places a card's contents as *siblings* of the card (which is
//!   what an absolutely-positioned canvas painter does) gives every one of them
//!   the canvas as their ancestor. "Is this run inside its owner" then means
//!   "is it inside the canvas", which is true of everything.
//! * `scene/pointer_reach` asks whether a widget can be pressed, which a run
//!   that escapes its card usually still can be.
//!
//! # The owner is the parent, and that is a demand on the tree
//!
//! A node is drawn inside its parent, because that is what a box means. That
//! is the whole rule, and it is only as honest as the tree: a painter that
//! emits `[Container(card), Text(id), Text(field), …]` as one flat sibling list
//! has thrown the containment away, and no walk can recover it.
//!
//! ★ A second rule was written and then **measured and deleted**. Tags in this
//! tree are addresses, so `lab.node.T-01.id` appears to say in its own name
//! that it is part of `lab.node.T-01`, and judging a mark against its longest
//! tag-prefix would have caught a flat painter without any restructuring. Run
//! against the real screen it produced two findings and **both were false**:
//! `lab.toolbar.zoom.out` is the button *beside* the readout `lab.toolbar.zoom`,
//! not content inside it. A dotted tag expresses grouping, not containment, and
//! a rule built on that reads a naming habit as a geometric promise.
//!
//! So the repair for a flat painter is to stop being one — to put the
//! containment back in the scene, where §2 #7 says the description of the
//! screen lives. This module then sees it. The limit is stated rather than
//! papered over: **a scene that lies about its structure gets a clean report
//! here**, and the round that found this fixed the painter rather than adding a
//! second channel to work around it.
//!
//! # Clipped is not better than smeared
//!
//! [`Fate::Smeared`] is a mark drawn on top of whatever is next to it.
//! [`Fate::Clipped`] is a mark whose overhang an enclosing clip cut away. They
//! are reported separately because the repairs differ — one is a layout fix,
//! the other is a policy choice (elide, wrap, scroll) — but neither is
//! acceptable silently: a clip turns "this label is too long" into "this label
//! ends here", and the reader cannot tell the difference.
//!
//! # What the reference toolkit can answer, measured at 6.11
//!
//! The geometry aggregates exist and the ink one does not. A widget's
//! `childrenRect` is the union of its children's **geometry** (measured: a
//! label given a 60x14 box reports 60x14 there while its text measures 251x17),
//! and a scene item's `childrenBoundingRect` likewise reports boxes. Ink is
//! available only one call at a time, from a font-metrics helper the caller has
//! to remember to invoke against a rectangle it has to fetch separately.
//! Nothing compares the two, nothing warns when a child leaves its parent, and
//! a child that does is silently clipped to it — `visibleRegion` reports the
//! *survivor*, not the loss. An external driver cannot ask at all.
//!
//! Here the whole answer is a pure function of the painted scene plus one
//! measurement closure, so it is one wire read, a paint-time warning, and a
//! gate every surface pays at boot.
//!
//! # Why the measurement is a closure
//!
//! The same reason [`crate::text_elide`] takes one: how wide a string is has
//! two legitimate answers in this project — shaped pixels on the GPU path,
//! terminal cells on the §2 #6 dual — and they cannot be reconciled. The
//! *policy* (which box owns which mark, and what counts as escaping it) is
//! shared here; the metric is handed in.

use std::collections::HashMap;

use crate::scene::{Rect, Scene, TextNode};

/// The height a box must have to hold one line of a `px` face without the
/// glyphs leaving it — a **reservation**, not a measurement.
///
/// ★ R1656 — this exists because the commonest authoring mistake this module
/// catches is a box authored at the *font size*. A shaped line box is taller
/// than the face it holds (ascent, descent and leading are all above and below
/// the em), so a 12px label in a 12px box overflows by construction, and that
/// is why `scene/text_painted`'s "the ink is bigger than the box" answered
/// `true` for 124 of 157 runs on the first screen it was pointed at.
///
/// Deliberately **conservative** — it reserves more than any face this project
/// ships needs. That direction is the safe one: reserving too much wastes a
/// pixel, reserving too little paints over the neighbour. A caller that wants
/// the exact number for a real face asks the shaper
/// (`pinion_text::LayoutCache::ink_size`), which the wire read does; this is
/// for a view function, which is sync and has no cache.
#[must_use]
pub const fn line_box(px: u32) -> u32 {
    px * 3 / 2 + 2
}

/// How far a mark reached past the box that owns it, per edge, in pixels.
///
/// Four numbers rather than one boolean because the boolean was measured
/// useless: on the screen this module was written for, "the ink is bigger than
/// the box" is true of 79% of runs. *How much* and *which edge* is what tells a
/// three-pixel line-box rounding from a row painted over its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overhang {
    /// Pixels past the owner's left edge.
    pub left: u32,
    /// Pixels past the owner's top edge.
    pub top: u32,
    /// Pixels past the owner's right edge.
    pub right: u32,
    /// Pixels past the owner's bottom edge.
    pub bottom: u32,
}

impl Overhang {
    /// Nothing escaped.
    pub const NONE: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    /// How far `inner` reaches past `outer` on each edge.
    #[must_use]
    pub fn of(inner: Rect, outer: Rect) -> Self {
        Self {
            left: outer.x.saturating_sub(inner.x),
            top: outer.y.saturating_sub(inner.y),
            right: (inner.x + inner.w).saturating_sub(outer.x + outer.w),
            bottom: (inner.y + inner.h).saturating_sub(outer.y + outer.h),
        }
    }

    /// True when the mark stayed inside.
    #[must_use]
    pub const fn is_contained(&self) -> bool {
        self.left == 0 && self.top == 0 && self.right == 0 && self.bottom == 0
    }

    /// The largest single-edge overhang — what a budget counts and what a
    /// tolerance compares against.
    #[must_use]
    pub const fn worst(&self) -> u32 {
        let a = if self.left > self.top {
            self.left
        } else {
            self.top
        };
        let b = if self.right > self.bottom {
            self.right
        } else {
            self.bottom
        };
        if a > b { a } else { b }
    }
}

/// What happened to the part that did not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// Nothing cut it: it is painted on top of whatever is beside the owner.
    Smeared,
    /// An enclosing clip removed it. The reader loses the content with no mark
    /// that anything was removed — which is why this is reported rather than
    /// forgiven.
    Clipped,
}

impl Fate {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Smeared => "smeared",
            Self::Clipped => "clipped",
        }
    }
}

/// One painted mark that did not stay inside the box that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escape {
    /// The mark's own tag, when it has one. A text run usually does not, which
    /// is exactly why this class stayed invisible: every other gate here is
    /// tag-keyed.
    pub tag: Option<String>,
    /// The mark's address, as `scene/locate` reports it.
    pub path: Vec<String>,
    /// The tag of the box it escaped, or `<untagged>` when that box has no tag.
    pub owner: String,
    /// What the mark holds, for a text run — the string a reader is losing.
    pub content: Option<String>,
    /// Window-absolute box the scene promised the mark.
    pub promised: Rect,
    /// Window-absolute extent actually painted: the shaped ink for a text run,
    /// the promised box for anything else.
    pub painted: Rect,
    /// Window-absolute box of the owner.
    pub owner_rect: Rect,
    /// How far past each edge of the owner the paint reached.
    pub over: Overhang,
    /// Whether the overhang is cut by a clip or drawn over the neighbour.
    pub fate: Fate,
}

/// How wide and tall a text run's glyphs actually are, in the caller's unit.
///
/// Handed in rather than computed here for the reason the module header gives:
/// the GPU path measures shaped pixels and the §2 #6 terminal measures cells,
/// and no single answer is right for both.
pub type InkOf<'a> = &'a mut dyn FnMut(&TextNode) -> (u32, u32);

/// Every painted mark that left the box that owns it, in paint order.
///
/// The walk is [`Scene::for_each_node`], so the geometry fold — enclosing
/// scroll offsets and clips — is the same one the tag resolver and the hit test
/// use. A caller cannot get a different answer by doing the arithmetic itself,
/// which is the failure R1653 recorded when three descents each folded their
/// own.
///
/// # Precondition
///
/// The scene has been through `pinion_runtime::compute_layout`: every node's
/// `rect` is in its enclosing scroll frame, not relative to its parent. That is
/// what the renderer and the hit test read, so it is the only frame in which
/// "inside" is the question a reader is asking. Handed an un-laid-out tree this
/// reports whatever the author happened to write down, which is why the
/// consumer-side tests here drive `view()` through the real layout pass rather
/// than asserting against hand-written rectangles.
///
/// `ink_of` is asked only about [`Scene::Text`] nodes. Every other kind paints
/// its own rectangle, so its promise and its paint are the same value and it
/// can only escape by being placed outside its owner — which is still worth
/// reporting, and is how a badge drawn past its card is caught.
#[must_use]
pub fn escapes(scene: &Scene, ink_of: InkOf<'_>) -> Vec<Escape> {
    // Pass 1 — every node's window-absolute rectangle, keyed by identity.
    //
    // A parent is always visited before its children, so a one-pass version
    // would work for the LOOKUP; it is two passes because reading a parent's
    // rectangle out of the child's own fold is what the first draft did and it
    // was wrong. `NodeVisit::offset` accumulates only what enclosing
    // [`Scene::Scroll`] nodes contribute — post-layout rectangles are already
    // in their scroll frame, not their parent's — so subtracting the parent's
    // origin back out (which is what "the parent's rect is in its parent's
    // frame" would require) double-counted it and reported overhangs in the
    // thousands of pixels for a glyph inside a button. Measured, on the first
    // run against a real screen.
    let mut absolute: HashMap<*const Scene, Rect> = HashMap::new();
    scene.for_each_node(&mut |visit| {
        absolute.insert(
            std::ptr::from_ref(visit.node),
            translate(visit.node.rect(), visit.offset),
        );
    });

    // Pass 2 — the judgment.
    let mut found = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Some(parent) = visit.ancestors.last() else {
            return; // the root answers to nothing
        };
        let Some(&owner_rect) = absolute.get(&std::ptr::from_ref(*parent)) else {
            return;
        };
        if owner_rect.w == 0 || owner_rect.h == 0 {
            // A parent with no extent is a grouping node, not a box: it makes
            // no promise about where its children go, so it cannot be broken.
            return;
        }
        if matches!(parent, Scene::Scroll(_)) {
            // A scroll's content is SUPPOSED to be bigger than the viewport —
            // that is what makes it scrollable. Judging it here reported a
            // world surface as a 4,476-pixel escape on the first real run, and
            // a check that fires on the normal case is a check nobody keeps.
            // Marks INSIDE that content are still judged against their own
            // boxes, which is where the question is meaningful.
            return;
        }
        // Where this mark sits with no clip folded in: `absolute_rect` answers
        // where it can be SEEN, and a mark whose overhang is entirely clipped
        // away has still been mis-placed. The clip is read separately below,
        // to decide the fate rather than to hide the escape.
        let promised = translate(visit.node.rect(), visit.offset);
        let (painted, content) = match visit.node {
            Scene::Text(t) => {
                let (w, h) = ink_of(t);
                (
                    Rect::new(promised.x, promised.y, w, h),
                    Some(t.content.clone()),
                )
            }
            _ => (promised, None),
        };
        if painted.w == 0 || painted.h == 0 {
            return; // nothing was drawn, so nothing left anything
        }
        let over = Overhang::of(painted, owner_rect);
        if over.is_contained() {
            return;
        }
        let fate = match visit.clip {
            Some(clip) if !Overhang::of(painted, clip).is_contained() => Fate::Clipped,
            _ => Fate::Smeared,
        };
        found.push(Escape {
            tag: visit.node.tag().map(str::to_owned),
            path: visit.path.to_vec(),
            owner: parent
                .tag()
                .map_or_else(|| UNTAGGED.to_owned(), str::to_owned),
            content,
            promised,
            painted,
            owner_rect,
            over,
            fate,
        });
    });
    found
}

/// What an escape names as its owner when the box that broke its promise
/// carries no tag. Spelled once so a caller filtering on it and this module
/// producing it cannot drift.
pub const UNTAGGED: &str = "<untagged>";

/// Fold a container-local rectangle into the walk root's frame.
///
/// Saturating rather than wrapping: R1653 measured what an underflow here costs
/// — a pan to the left turned a `u32` subtraction into a coordinate near four
/// billion, which is a panic in a debug build and a silently absurd rectangle
/// in a release one.
fn translate(rect: Rect, offset: (i64, i64)) -> Rect {
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "clamped into u32's range on the line above the cast"
    )]
    let fold =
        |v: u32, by: i64| -> u32 { (i64::from(v) + by).clamp(0, i64::from(u32::MAX)) as u32 };
    Rect::new(
        fold(rect.x, offset.0),
        fold(rect.y, offset.1),
        rect.w,
        rect.h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
    use crate::style::BoxStyle;

    /// A text run whose ink the fixture decides, so these tests are about the
    /// POLICY and never about a shaper.
    fn text(content: &str, rect: Rect, tag: Option<&'static str>) -> Scene {
        let node = TextNode::new(content, rect);
        Scene::Text(match tag {
            Some(t) => node.with_tag(t),
            None => node,
        })
    }

    fn boxed(rect: Rect, tag: &str, children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = rect;
        c.tag = Some(tag.to_owned().into());
        Scene::Container(c)
    }

    /// Ink measured as `len * 8` wide by `12` tall — a stand-in with no font in
    /// it, so the assertions are about which box was compared against.
    fn stub_ink(t: &TextNode) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "fixture strings are a handful of characters"
        )]
        let w = (t.content.chars().count() as u32) * 8;
        (w, 12)
    }

    /// ★ The defect this module was written for, as a property: a card whose
    /// last row is painted below its own border.
    ///
    /// Stated against the ink rather than the box because that is the half that
    /// was missing — the row's promised rectangle can sit inside the card while
    /// the glyphs it holds do not.
    #[test]
    fn r1656_a_row_painted_past_the_card_border_is_reported() {
        let card = boxed(
            Rect::new(10, 10, 100, 40),
            "card",
            vec![text("row", Rect::new(14, 40, 40, 11), None)],
        );
        let found = escapes(&card, &mut stub_ink);
        assert_eq!(found.len(), 1, "one escape, not two: {found:?}");
        let escape = &found[0];
        assert_eq!(escape.owner, "card");
        assert_eq!(escape.content.as_deref(), Some("row"));
        // Row top at 40, ink 12 tall -> 52; the card's bottom edge is 10+40=50.
        assert_eq!(escape.over.bottom, 2, "{escape:?}");
        assert_eq!(escape.over.right, 0);
        assert_eq!(escape.fate, Fate::Smeared, "nothing clipped it");
    }

    /// ★ The stated limit, as a test: a painter that flattens its containment
    /// gets a CLEAN report, and that is not this module being wrong.
    ///
    /// This is the shape the analysis-tool canvas actually painted — the card
    /// and its parts as siblings — and it is why every check in the tree
    /// answered "contained" while a person could see text outside the border.
    /// The repair is to put the relation back in the scene (§2 #7 says the
    /// scene IS the description of the screen), not to guess it from a naming
    /// habit: a rule that judged a mark against its longest tag-prefix was
    /// written, measured against the real screen, and produced two findings
    /// that were both false — `lab.toolbar.zoom.out` is the button BESIDE the
    /// readout `lab.toolbar.zoom`, not content inside it.
    ///
    /// Asserting the blindness is what keeps it from being rediscovered as a
    /// surprise, and what makes the nesting repair in the consumer load-bearing
    /// rather than cosmetic.
    #[test]
    fn r1656_a_flat_painter_gets_a_clean_report_and_that_is_the_limit() {
        let flat = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::new(Rect::new(0, 0, 100, 40), BoxStyle::default()).with_tag("card"),
            ),
            // Painted well below the card, as a SIBLING of it.
            text("id", Rect::new(4, 44, 40, 11), Some("card.id")),
        ]));
        let root_rect = Rect::new(0, 0, 1000, 1000);
        let mut rooted = ContainerNode::new(match flat {
            Scene::Container(c) => c.children,
            other => vec![other],
        });
        rooted.rect = root_rect;
        let found = escapes(&Scene::Container(rooted), &mut stub_ink);
        assert!(
            found.is_empty(),
            "the scene says both are children of the root, and they are: {found:?}"
        );

        // The same two marks, with the relation the painter meant present in
        // the tree: now the escape is visible, and it is the only difference.
        let nested = boxed(
            Rect::new(0, 0, 100, 40),
            "card",
            vec![text("id", Rect::new(4, 44, 40, 11), None)],
        );
        let found = escapes(&nested, &mut stub_ink);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].owner, "card");
        assert_eq!(found[0].over.bottom, 16, "{:?}", found[0]);
    }

    /// ★ A mark inside its box is not reported, however tight the fit — the
    /// meter this replaces failed exactly here, reporting 79% of a screen.
    #[test]
    fn r1656_a_mark_that_fits_exactly_is_not_an_escape() {
        let card = boxed(
            Rect::new(0, 0, 24, 12),
            "card",
            vec![text("abc", Rect::new(0, 0, 24, 12), None)],
        );
        assert!(
            escapes(&card, &mut stub_ink).is_empty(),
            "3 chars * 8 = 24 wide, 12 tall, in a 24x12 box"
        );
    }

    /// ★ A clip does not make an escape acceptable, it changes what the reader
    /// loses: the content is gone with nothing saying so.
    #[test]
    fn r1656_a_clipped_overhang_is_reported_as_clipped() {
        let inner = boxed(
            Rect::new(0, 0, 40, 20),
            "card",
            vec![text("a much longer string", Rect::new(0, 0, 40, 12), None)],
        );
        let scroll = Scene::Scroll(crate::scene::ScrollNode::new(
            Rect::new(0, 0, 40, 20),
            inner,
        ));
        let found = escapes(&scroll, &mut stub_ink);
        let cut: Vec<_> = found.iter().filter(|e| e.fate == Fate::Clipped).collect();
        assert!(
            !cut.is_empty(),
            "the scroll's clip cuts the overhang, and that is still a loss: {found:?}"
        );
    }

    /// ★ A scroll's content is allowed to exceed its viewport, because that is
    /// what makes it scrollable — and the marks inside that content are still
    /// judged.
    #[test]
    fn r1656_scroll_content_is_not_an_escape_but_its_marks_still_are() {
        let card = boxed(
            Rect::new(0, 0, 100, 40),
            "card",
            vec![text("row", Rect::new(0, 44, 40, 11), None)],
        );
        let mut world = ContainerNode::new(vec![card]);
        world.rect = Rect::new(0, 0, 4000, 4000);
        let scroll = Scene::Scroll(crate::scene::ScrollNode::new(
            Rect::new(0, 0, 200, 200),
            Scene::Container(world),
        ));
        let found = escapes(&scroll, &mut stub_ink);
        assert_eq!(
            found.len(),
            1,
            "the 4000px world is not an escape; the row below its card is: {found:?}"
        );
        assert_eq!(found[0].owner, "card");
    }

    /// ★ The overhang is per edge, because which edge it is decides the repair.
    #[test]
    fn r1656_the_overhang_names_the_edge() {
        let over = Overhang::of(Rect::new(5, 5, 100, 100), Rect::new(10, 10, 50, 50));
        assert_eq!(over.left, 5);
        assert_eq!(over.top, 5);
        assert_eq!(over.right, 45);
        assert_eq!(over.bottom, 45);
        assert_eq!(over.worst(), 45);
        assert!(!over.is_contained());
        assert!(Overhang::of(Rect::new(10, 10, 5, 5), Rect::new(10, 10, 50, 50)).is_contained());
    }
}
