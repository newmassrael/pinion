//! R1801 §5.32 §2 #7 — **where a side panel lives, as a value that can change
//! and a declaration that can refuse.**
//!
//! # The report this answers
//!
//! A reader asked, three times across eleven rounds, why the node palette and
//! the node inspector cannot be moved. The measured answer was that the screen
//! lays them out as two fixed-width columns of a hand-written strip: they have
//! addresses and no gesture, and `scene/drop_targets` answers `clauses: []` for
//! that surface — nothing had ever declared them movable.
//!
//! This is the vocabulary that lets a panel say where it may live, so that
//! moving it is a *value changing* rather than a layout being rewritten.
//!
//! # Why not [`crate::region`]
//!
//! That module is about geometric selection — rectangles, spans and lassos, and
//! whether a point is inside one. The reference editor this axis is measured
//! against calls a panel-inside-an-area a "region" too, and adopting the word
//! here would give one name in one crate two unrelated meanings. This module is
//! named for what it holds: a panel attached to an EDGE.
//!
//! # ★★★★★ What the floor does, measured, and where it stops
//!
//! The floor toolkit has a first-class dockable panel, so the question was
//! never *does this exist* but exactly what it gives. Built from source and run
//! offscreen at R1801:
//!
//! * **Where a panel is IS readable** — four edges as an enum, a floating flag,
//!   and a signal when it lands somewhere new. This module owes at least that,
//!   and [`EdgePlacement`] is that.
//! * **Folding does not exist.** Of 104 published member names on that class and
//!   its bases, zero name collapse, fold, expand or toggle. Hiding a panel
//!   removes it from the layout — it is *gone*, not folded to a strip a reader
//!   can grab. So [`EdgePlacement::folded`] has no counterpart there, and the
//!   editor this axis comes from treats fold as the primary gesture on a side
//!   region.
//! * **The arrangement round-trips as OPAQUE BYTES** — 82 of them in the probe,
//!   keyed by object name. Nothing can read which edge a panel is on out of it,
//!   diff two of them, or write one by hand. This project already decided the
//!   other way for tile layouts (R1607/R1608, "a layout is a value that
//!   round-trips"), and [`EdgePlacement`] is data for the same reason.
//! * 🟥 **A refused move is SILENT.** Restricting a panel to the left edge and
//!   then asking for the right edge put it on the right: the declaration was
//!   ignored, with nothing thrown, nothing returned and no signal.
//!
//! ## And that last one is a habit, not an accident
//!
//! Three consecutive rounds measured the same shape on three unrelated axes:
//! an explicit panel minimum silently overriding a declared shrink policy
//! (R1798), an explicit box height silently overriding the minimum a face needs
//! (R1800), and now an explicit move silently overriding the edges a panel
//! declared it may occupy. ⇒ **on the floor, a declared constraint loses to an
//! imperative call, quietly.**
//!
//! This module goes the other way, and that is the whole of its claim to being
//! better: [`EdgePolicy::admit`] returns a [`Result`], the error names the edge
//! that was asked for AND the edges that were allowed, and it carries a
//! [`RefusalReason`] — the same type an `External` refusal carries, so a panel
//! that will not move explains itself in the vocabulary the rest of this tree
//! already speaks and a person can be shown the sentence.

use crate::external::RefusalReason;
use crate::style::ChromeEdge;

/// Where a panel is, and how much room it takes there.
///
/// A value: cloneable, comparable, and readable field by field. It is the whole
/// arrangement of one panel, so a screen's arrangement is a list of these and a
/// reader can diff two of them without decoding anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EdgePlacement {
    /// Which edge of its host it is attached to.
    pub edge: ChromeEdge,
    /// Its thickness across that edge, in logical pixels, when open.
    ///
    /// Kept even while [`Self::folded`], so unfolding restores the size the
    /// reader had chosen rather than a default. That is the difference between
    /// folding and hiding, and it is the point.
    pub extent: u32,
    /// Whether it is folded to its strip.
    pub folded: bool,
}

impl EdgePlacement {
    /// A panel open at `edge`, `extent` thick.
    #[must_use]
    pub const fn open(edge: ChromeEdge, extent: u32) -> Self {
        Self {
            edge,
            extent,
            folded: false,
        }
    }

    /// ★★★★★ (R1902) A panel **folded to its strip** at `edge`, keeping
    /// `extent` for when it is opened.
    ///
    /// The peer of [`Self::open`], and it exists because *starting folded* is a
    /// placement rather than an act. The reference editor this campaign reads
    /// declares its tool region hidden on the region TYPE, so a panel that
    /// begins out of the way is a property of the panel and not a gesture
    /// somebody performed before the reader arrived.
    ///
    /// ⚠ `extent` is kept, not zeroed, for the reason [`Self::extent`] gives:
    /// opening a panel that started folded must give it a size worth having,
    /// and a fold that forgot its extent would open to nothing. That is the
    /// difference between folding and hiding, at the one moment the difference
    /// is easiest to lose.
    #[must_use]
    pub const fn folded_at(edge: ChromeEdge, extent: u32) -> Self {
        Self {
            edge,
            extent,
            folded: true,
        }
    }

    /// How much room this placement actually takes, given the strip a folded
    /// panel keeps.
    ///
    /// ★ A folded panel is NOT zero. The floor's nearest gesture is `hide`,
    /// which takes the panel out of the layout entirely and leaves a reader
    /// with nothing to click to bring it back. A fold leaves the strip, which
    /// is what makes it reversible by the person who did it rather than only
    /// by a menu somewhere else.
    #[must_use]
    pub const fn thickness(self, strip: u32) -> u32 {
        if self.folded { strip } else { self.extent }
    }

    /// Whether this placement lies along the horizontal axis (left or right),
    /// which is what decides whether `extent` is a width or a height.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self.edge, ChromeEdge::Left | ChromeEdge::Right)
    }
}

/// ★★★★★ R1889 — **whether a reader may drag this panel's
/// [`extent`](EdgePlacement::extent), and between what.**
///
/// The third of the reference editor's three region operators. R1887 gave a
/// panel its edge (`region_flip`) and its fold (`region_toggle`); this is
/// `region_scale`, and it was the field of [`EdgePlacement`] nothing could
/// change — measured at R1889, the tree held **zero** writers of `extent`
/// outside the two constants that seed it.
///
/// # Why an enum with two arms and no third
///
/// Because [`Fixed`](Self::Fixed) has to be a CLAIM, not a gap. A panel whose
/// width the specification settles is a real and common thing — this screen's
/// rail is one — and the honest way to say so is an arm that says it. What must
/// not exist is a way for a panel to arrive here having said *nothing*, because
/// then "the author did not think about it" and "this width is deliberate" are
/// one value and a gate cannot tell them apart. There are two arms and no
/// catch-all, so every panel has answered.
///
/// ⚠ [`EdgePolicy::movable`] gives `Fixed`, and that is the same argument one
/// level up rather than a silent default: a panel that may change EDGE and not
/// SIZE is a coherent declaration, and the alternative — inventing bounds for a
/// panel whose author never named any — would be the framework guessing a
/// number a reader can then drag to. Say [`EdgePolicy::resizable`] to mean it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resize {
    /// The specification settles this panel's extent; a reader cannot drag it.
    Fixed,
    /// A reader may drag the extent anywhere in `min..=max`, inclusive.
    Between {
        /// The narrowest the panel may be dragged to.
        min: u32,
        /// The widest.
        max: u32,
    },
}

impl Resize {
    /// The extent a DRAG should ask for, given where the pointer went — or
    /// `None` when this panel does not resize at all.
    ///
    /// ★★★★★ Why clamping lives in the policy rather than in the gesture. A
    /// drag and a wire call want opposite things from a value out of range: a
    /// hand sliding past the minimum means *as narrow as it goes*, while an
    /// agent asking for 40 has made an error it needs told about. Writing the
    /// clamp in the screen would put the bound in two places — here and there —
    /// and the two would drift, which is the failure this whole module exists
    /// to stop one property over.
    ///
    /// So the policy owns both readings: a drag clamps and is then admitted (it
    /// can no longer be refused, because it no longer asks for anything out of
    /// range), and a wire call goes straight to
    /// [`EdgePolicy::admit_extent`] and is refused with the bounds. One
    /// declaration, two behaviours, and neither is a second copy of the rule.
    #[must_use]
    pub const fn clamp(self, extent: u32) -> Option<u32> {
        match self {
            Resize::Fixed => None,
            Resize::Between { min, max } => {
                if extent < min {
                    Some(min)
                } else if extent > max {
                    Some(max)
                } else {
                    Some(extent)
                }
            }
        }
    }

    /// Whether a reader may drag this panel at all.
    #[must_use]
    pub const fn is_draggable(self) -> bool {
        matches!(self, Resize::Between { .. })
    }
}

/// What a panel is allowed to do — declared once, beside the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgePolicy {
    /// The edges this panel may occupy. Empty means it may not move at all,
    /// which is a legitimate declaration and not a mistake — a rail pinned to
    /// one side says so this way.
    pub allowed: &'static [ChromeEdge],
    /// Whether it may fold to its strip.
    pub foldable: bool,
    /// ★ R1889 — whether a reader may drag its extent, and between what.
    pub resize: Resize,
}

/// Why a placement change was refused.
///
/// ★ Carries what was ASKED and what was ALLOWED, not just a failure. A refusal
/// a person cannot act on is a refusal that gets worked around, and this tree's
/// standing rule since R1706 is that a refusal reaches a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeRefusal {
    /// The panel does not admit that edge.
    EdgeNotAllowed {
        /// The edge the caller asked for.
        asked: ChromeEdge,
        /// The edges the panel declared it admits.
        allowed: &'static [ChromeEdge],
    },
    /// The panel declared it does not fold.
    NotFoldable,
    /// ★ R1889 — the panel's extent is the specification's, so there is nothing
    /// to drag.
    NotResizable,
    /// ★ R1889 — the panel resizes, and not that far.
    ///
    /// Carries the bounds for the reason [`EdgeNotAllowed`](Self::EdgeNotAllowed)
    /// carries the allowed edges: *no* is not actionable and *no, between 180
    /// and 420* is.
    ExtentOutOfRange {
        /// The extent the caller asked for.
        asked: u32,
        /// The narrowest this panel goes.
        min: u32,
        /// The widest.
        max: u32,
    },
}

impl EdgeRefusal {
    /// The sentence, in the vocabulary an `External` refusal already uses.
    #[must_use]
    pub fn reason(&self) -> RefusalReason {
        match self {
            Self::EdgeNotAllowed { .. } => {
                RefusalReason::stated("this panel does not admit that edge")
            }
            Self::NotFoldable => RefusalReason::stated("this panel does not fold"),
            // ★ R1889 — the bounds are IN the sentence, not only in the arm. A
            // refusal reaches a person as words (R1706), and "this panel does
            // not go that wide" leaves them guessing at the number they should
            // have asked for.
            Self::NotResizable => {
                RefusalReason::stated("this panel's width is fixed by its specification")
            }
            Self::ExtentOutOfRange { min, max, .. } => RefusalReason::from(format!(
                "this panel resizes between {min} and {max} logical pixels"
            )),
        }
    }

    /// A short machine word for the wire, so a client can branch without
    /// parsing the sentence.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::EdgeNotAllowed { .. } => "edge-not-allowed",
            Self::NotFoldable => "not-foldable",
            Self::NotResizable => "not-resizable",
            Self::ExtentOutOfRange { .. } => "extent-out-of-range",
        }
    }
}

impl EdgePolicy {
    /// A panel that may sit on any of `allowed` and folds.
    #[must_use]
    pub const fn movable(allowed: &'static [ChromeEdge]) -> Self {
        Self {
            allowed,
            foldable: true,
            // ★ R1889 — and its width is the specification's until somebody
            // says otherwise with `resizable`. See [`Resize`] for why this is a
            // claim rather than a default nobody made.
            resize: Resize::Fixed,
        }
    }

    /// ★ R1889 — the same panel, with an extent a reader may drag between
    /// `min` and `max`.
    ///
    /// A builder rather than a fourth constructor, because *may it move* and
    /// *may it be resized* are independent: a panel can be draggable in size
    /// and pinned to one edge, or the reverse, and a constructor per
    /// combination is the shape that stops scaling at the third property.
    #[must_use]
    pub const fn resizable(self, min: u32, max: u32) -> Self {
        Self {
            resize: Resize::Between { min, max },
            ..self
        }
    }

    /// A panel that stays where it is put and does not fold — a rail.
    #[must_use]
    pub const fn fixed() -> Self {
        Self {
            allowed: &[],
            foldable: false,
            resize: Resize::Fixed,
        }
    }

    /// Whether this policy admits `edge`.
    #[must_use]
    pub fn admits(&self, edge: ChromeEdge) -> bool {
        self.allowed.contains(&edge)
    }

    /// The placement that results from asking to move to `edge`, or the refusal.
    ///
    /// ★ A `Result`, deliberately, and this is the module's whole argument. The
    /// floor's equivalent accepts the move and ignores the declaration: measured
    /// at R1801, a panel restricted to the left edge and then asked for the
    /// right edge ends up on the right, with nothing thrown, nothing returned
    /// and no signal. A constraint that can be walked past in silence is not a
    /// constraint, it is a comment.
    ///
    /// # Errors
    ///
    /// [`EdgeRefusal::EdgeNotAllowed`] when the policy does not admit `edge`,
    /// carrying both the edge asked for and the edges that were allowed.
    pub fn admit(
        &self,
        from: EdgePlacement,
        edge: ChromeEdge,
    ) -> Result<EdgePlacement, EdgeRefusal> {
        if !self.admits(edge) {
            return Err(EdgeRefusal::EdgeNotAllowed {
                asked: edge,
                allowed: self.allowed,
            });
        }
        Ok(EdgePlacement { edge, ..from })
    }

    /// The placement that results from folding or unfolding, or the refusal.
    ///
    /// ★ Unfolding is never refused, even by a policy that forbids folding: a
    /// panel that somehow arrived folded must be able to come back, or a
    /// declaration change strands a reader with a strip they cannot open. Only
    /// the fold itself is gated.
    ///
    /// # Errors
    ///
    /// [`EdgeRefusal::NotFoldable`] when asked to fold a panel that declared it
    /// does not fold.
    pub fn admit_fold(
        &self,
        from: EdgePlacement,
        folded: bool,
    ) -> Result<EdgePlacement, EdgeRefusal> {
        if folded && !self.foldable {
            return Err(EdgeRefusal::NotFoldable);
        }
        Ok(EdgePlacement { folded, ..from })
    }

    /// ★★★★★ R1889 — the placement that results from asking for `extent`, or
    /// the refusal.
    ///
    /// The peer of [`admit`](Self::admit) and [`admit_fold`](Self::admit_fold),
    /// and the last of the three region operators this module owes. It REFUSES
    /// rather than clamping, which is the half a wire caller needs: an agent
    /// that asked for 40 has made a mistake, and silently getting 180 is how it
    /// goes on believing 40 worked.
    ///
    /// A dragging hand wants the other reading, and gets it by asking
    /// [`Resize::clamp`] first — see that method for why the clamp belongs to
    /// the policy rather than to the gesture.
    ///
    /// # Errors
    ///
    /// [`EdgeRefusal::NotResizable`] when the panel declared
    /// [`Resize::Fixed`], and [`EdgeRefusal::ExtentOutOfRange`] — carrying the
    /// bounds — when it resizes and not that far.
    pub fn admit_extent(
        &self,
        from: EdgePlacement,
        extent: u32,
    ) -> Result<EdgePlacement, EdgeRefusal> {
        match self.resize {
            Resize::Fixed => Err(EdgeRefusal::NotResizable),
            Resize::Between { min, max } if extent < min || extent > max => {
                Err(EdgeRefusal::ExtentOutOfRange {
                    asked: extent,
                    min,
                    max,
                })
            }
            Resize::Between { .. } => Ok(EdgePlacement { extent, ..from }),
        }
    }

    /// ★★★★★ (R1902) The placement a panel **opens in**, judged by the same
    /// rules a gesture is judged by — or the refusal.
    ///
    /// # The hole this closes
    ///
    /// Measured at R1902: every *change* to a placement passed through
    /// [`admit`](Self::admit), [`admit_fold`](Self::admit_fold) or
    /// [`admit_extent`](Self::admit_extent), and the placement a screen
    /// **started in** passed through nothing at all. It was a constant. So a
    /// panel could declare `foldable: false` and open folded, or open on an
    /// edge its own `allowed` list does not contain, and nothing anywhere would
    /// say a word — the declaration and the opening state were free to
    /// contradict each other for the whole life of the program.
    ///
    /// That is a sharper form of the habit this module was built against. The
    /// floor toolkit lets an imperative call quietly beat a declared
    /// constraint; here the *initial* state never met its declaration at all,
    /// which is worse, because there is no call to blame and no moment to
    /// watch.
    ///
    /// # Why it composes rather than re-spells
    ///
    /// The three checks are the three that already exist, called in turn. A
    /// fourth spelling of "is this edge allowed" is a fourth answer, and this
    /// tree has paid for that shape often enough to have a rule about it: a
    /// gate compares against the *output of the derivation*, it does not write
    /// the derivation again.
    ///
    /// # The two things it does not check, and why neither is an exemption
    ///
    /// A [`Resize::Fixed`] panel's extent is checked against nothing, because
    /// there is nothing to check it against — `Fixed` declares no range, so the
    /// opening extent is the only extent and cannot be outside anything. That
    /// is the ABSENCE of a predicate, not an exemption from one; a `Fixed`
    /// panel whose extent were checked with [`admit_extent`](Self::admit_extent)
    /// would be refused for having any extent at all.
    ///
    /// ★★★★★ A panel with an EMPTY [`allowed`](Self::allowed) list has its
    /// opening edge checked against nothing either, and this one the gate found
    /// on its first run against a real specification: the first draft refused
    /// `lab.rail`, which is pinned to the left and admits no edge. `allowed:
    /// []` says *this cannot be MOVED*, and that field's own documentation says
    /// so — "a rail pinned to one side says so this way". A pinned panel is
    /// still ON a side. Refusing it would have made the declaration mean *this
    /// panel is nowhere*, which is not a state a screen can be in.
    ///
    /// It is not a way to dodge the gate, because emptying `allowed` is a loud
    /// declaration with consequences of its own — no flip control, and an empty
    /// drop contract on the wire — and the FOLD check still applies to a pinned
    /// panel, which is the case this gate exists for.
    ///
    /// # Errors
    ///
    /// [`EdgeRefusal::EdgeNotAllowed`] when the opening edge is not admitted;
    /// [`EdgeRefusal::NotFoldable`] when it opens folded and may not fold;
    /// [`EdgeRefusal::ExtentOutOfRange`] when a resizable panel opens outside
    /// its own declared range.
    pub fn admit_opening(&self, opening: EdgePlacement) -> Result<EdgePlacement, EdgeRefusal> {
        let placed = if self.allowed.is_empty() {
            opening
        } else {
            self.admit(opening, opening.edge)?
        };
        let placed = if opening.folded {
            self.admit_fold(placed, true)?
        } else {
            placed
        };
        match self.resize {
            Resize::Fixed => Ok(placed),
            Resize::Between { .. } => self.admit_extent(placed, opening.extent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EdgePlacement, EdgePolicy, EdgeRefusal, Resize};
    use crate::style::ChromeEdge;

    const SIDES: &[ChromeEdge] = &[ChromeEdge::Left, ChromeEdge::Right];

    #[test]
    fn r1801_a_panel_moves_between_the_edges_it_admits() {
        let policy = EdgePolicy::movable(SIDES);
        let at = EdgePlacement::open(ChromeEdge::Left, 230);
        let moved = policy
            .admit(at, ChromeEdge::Right)
            .expect("right is allowed");
        assert_eq!(moved.edge, ChromeEdge::Right);
        // ★ The extent travels with it: a reader who widened the palette does
        // not lose that by moving it to the other side.
        assert_eq!(moved.extent, 230);
        assert!(!moved.folded);
    }

    /// ★★★★★ The measured floor accepts this move and says nothing. This is the
    /// assertion that makes the difference real rather than claimed.
    #[test]
    fn r1801_a_move_the_panel_does_not_admit_is_refused_and_says_why() {
        let policy = EdgePolicy::movable(SIDES);
        let at = EdgePlacement::open(ChromeEdge::Left, 230);
        let refused = policy
            .admit(at, ChromeEdge::Top)
            .expect_err("top is not in the declared set");
        match refused {
            EdgeRefusal::EdgeNotAllowed { asked, allowed } => {
                assert_eq!(asked, ChromeEdge::Top);
                assert_eq!(allowed, SIDES, "the refusal names what WAS allowed");
            }
            // Named rather than a wildcard: a refusal variant added later must
            // land here as a compile error, not be silently absorbed by a `_`.
            // The whole point of this type is that a refusal SAYS which one it
            // is, so a test that stops distinguishing them defeats it.
            // ★ R1889 added two arms and this match is where the compiler said
            // so — which is the mechanism working. Named, still, for the reason
            // above.
            other @ (EdgeRefusal::NotFoldable
            | EdgeRefusal::NotResizable
            | EdgeRefusal::ExtentOutOfRange { .. }) => panic!("wrong refusal: {other:?}"),
        }
        assert_eq!(refused.wire_word(), "edge-not-allowed");
        assert!(!refused.reason().as_str().is_empty());
    }

    /// A panel that declares no edges cannot be moved anywhere — including to
    /// the edge it is already on, because asking is still asking.
    #[test]
    fn r1801_a_fixed_panel_admits_nothing() {
        let policy = EdgePolicy::fixed();
        let at = EdgePlacement::open(ChromeEdge::Left, 54);
        assert!(policy.admit(at, ChromeEdge::Left).is_err());
        assert!(policy.admit(at, ChromeEdge::Right).is_err());
        assert!(policy.admit_fold(at, true).is_err());
    }

    /// ★ Folding is not hiding: the strip stays, and so does the size to
    /// come back to. The floor has neither — hiding removes the panel from the
    /// layout and there is no fold at all.
    #[test]
    fn r1801_a_folded_panel_keeps_its_strip_and_its_size() {
        let policy = EdgePolicy::movable(SIDES);
        let open = EdgePlacement::open(ChromeEdge::Left, 230);
        assert_eq!(open.thickness(18), 230);

        let folded = policy.admit_fold(open, true).expect("it folds");
        assert!(folded.folded);
        assert_eq!(folded.thickness(18), 18, "a fold leaves the strip");
        assert_eq!(folded.extent, 230, "and remembers the size to come back to");

        let back = policy.admit_fold(folded, false).expect("and it comes back");
        assert_eq!(back.thickness(18), 230);
    }

    /// ★ Unfolding is never refused, even under a policy that forbids folding —
    /// otherwise tightening a declaration strands a reader with a strip that
    /// cannot open.
    #[test]
    fn r1801_a_panel_can_always_come_back_from_a_fold() {
        let mut folded = EdgePlacement::open(ChromeEdge::Left, 230);
        folded.folded = true;
        let strict = EdgePolicy {
            allowed: SIDES,
            foldable: false,
            resize: Resize::Fixed,
        };
        assert!(strict.admit_fold(folded, true).is_err(), "it may not fold");
        let back = strict
            .admit_fold(folded, false)
            .expect("but it may always unfold");
        assert!(!back.folded);
    }

    /// ★★★★★ R1889 — a fixed panel REFUSES a width rather than accepting one,
    /// which is this module's whole argument applied to its third property.
    ///
    /// Measured on the floor at R1801 for the EDGE property: a panel restricted
    /// to one edge and asked for another ends up on the other, with nothing
    /// thrown and nothing returned. This is the same shape for size, answered
    /// the other way.
    #[test]
    fn r1889_a_fixed_panel_refuses_a_width_and_says_why() {
        let at = EdgePlacement::open(ChromeEdge::Left, 230);
        let rail = EdgePolicy::fixed();

        assert_eq!(rail.admit_extent(at, 300), Err(EdgeRefusal::NotResizable));
        assert!(
            !rail.resize.is_draggable(),
            "and it says so before anybody asks, so a screen paints no grip"
        );
        assert_eq!(
            rail.resize.clamp(300),
            None,
            "a drag has nothing to clamp TO, which is a different answer from \
             clamping to the current width — the latter would let a grip exist \
             and do nothing"
        );
    }

    /// ★★★★★ R1889 — a resizable panel refuses a width OUT OF RANGE, and the
    /// refusal carries the range.
    ///
    /// `no` is not actionable; `no, between 180 and 420` is. Same rule as
    /// `EdgeNotAllowed` carrying the allowed edges.
    #[test]
    fn r1889_an_out_of_range_width_is_refused_with_its_bounds() {
        let at = EdgePlacement::open(ChromeEdge::Left, 230);
        let panel = EdgePolicy::movable(SIDES).resizable(180, 420);

        assert_eq!(
            panel.admit_extent(at, 40),
            Err(EdgeRefusal::ExtentOutOfRange {
                asked: 40,
                min: 180,
                max: 420
            })
        );
        assert_eq!(
            panel.admit_extent(at, 900),
            Err(EdgeRefusal::ExtentOutOfRange {
                asked: 900,
                min: 180,
                max: 420
            })
        );
        let sentence = panel
            .admit_extent(at, 40)
            .expect_err("just refused")
            .reason()
            .to_string();
        assert!(
            sentence.contains("180") && sentence.contains("420"),
            "the bounds are in the SENTENCE a reader is shown, not only in the \
             arm a program matches on: {sentence}"
        );

        let wider = panel.admit_extent(at, 300).expect("300 is in range");
        assert_eq!(wider.extent, 300);
        assert_eq!(
            (wider.edge, wider.folded),
            (at.edge, at.folded),
            "and asking for a width changes the width and nothing else"
        );
    }

    /// ★★★★★ R1889 — a DRAG clamps and is then admitted, so the same
    /// declaration serves both readings without either being a second copy.
    ///
    /// The pair this test exists for: `clamp` then `admit_extent` can never
    /// refuse, and `admit_extent` alone can. A screen that clamped for itself
    /// would hold the bounds in a second place, which is the drift this module
    /// refuses one property over.
    #[test]
    fn r1889_a_drag_clamps_where_a_wire_call_is_refused() {
        let at = EdgePlacement::open(ChromeEdge::Left, 230);
        let panel = EdgePolicy::movable(SIDES).resizable(180, 420);

        for asked in [0, 40, 179, 180, 300, 420, 421, 10_000] {
            let dragged = panel
                .resize
                .clamp(asked)
                .expect("a draggable panel always answers a drag");
            assert!(
                (180..=420).contains(&dragged),
                "a hand that slides past the bound gets the bound, not a refusal"
            );
            panel
                .admit_extent(at, dragged)
                .expect("★ and a clamped ask can never be refused");
        }

        assert!(
            panel.admit_extent(at, 40).is_err(),
            "while the same number straight off the wire IS refused — the two \
             readings are the point"
        );
    }

    /// The axis a placement lies along is what decides whether its extent is a
    /// width or a height, so it is asked rather than inferred at each site.
    #[test]
    fn r1801_the_axis_comes_from_the_edge() {
        assert!(EdgePlacement::open(ChromeEdge::Left, 10).is_horizontal());
        assert!(EdgePlacement::open(ChromeEdge::Right, 10).is_horizontal());
        assert!(!EdgePlacement::open(ChromeEdge::Top, 10).is_horizontal());
        assert!(!EdgePlacement::open(ChromeEdge::Bottom, 10).is_horizontal());
    }

    // ── R1902: the placement a panel OPENS in is judged too ─────────────────

    #[test]
    fn r1902_a_panel_can_be_declared_folded_and_keeps_the_size_it_will_open_to() {
        let shut = EdgePlacement::folded_at(ChromeEdge::Left, 230);
        assert!(shut.folded);
        assert_eq!(
            shut.extent, 230,
            "a fold that forgot its extent would open to nothing"
        );
        assert_eq!(shut.edge, ChromeEdge::Left);
        // The two constructors differ in exactly one field, which is what makes
        // "opens folded" a placement rather than a different kind of thing.
        assert_eq!(
            EdgePlacement {
                folded: false,
                ..shut
            },
            EdgePlacement::open(ChromeEdge::Left, 230)
        );
    }

    #[test]
    fn r1902_an_opening_placement_a_panel_admits_is_returned_unchanged() {
        let policy = EdgePolicy::movable(SIDES);
        let opening = EdgePlacement::folded_at(ChromeEdge::Left, 230);
        assert_eq!(
            policy.admit_opening(opening).expect("movable and foldable"),
            opening
        );
    }

    /// ★★★★★ The hole R1902 closes: a panel that declares it does not fold, and
    /// opens folded anyway. Before this, nothing anywhere asked.
    #[test]
    fn r1902_a_panel_that_cannot_fold_may_not_open_folded() {
        let rail = EdgePolicy::fixed();
        // ★★★★★ A pinned panel is still ON a side, so its opening EDGE is
        // admitted — the first draft refused this and the gate caught it
        // against the real specification. What is refused is the FOLD.
        assert!(
            rail.admit_opening(EdgePlacement::open(ChromeEdge::Left, 54))
                .is_ok(),
            "an empty allowed-set says it cannot MOVE, not that it is nowhere"
        );
        let refused = rail
            .admit_opening(EdgePlacement::folded_at(ChromeEdge::Left, 54))
            .expect_err("a fixed rail declares it does not fold");
        assert!(matches!(refused, EdgeRefusal::NotFoldable), "{refused:?}");

        // A panel that may move but may not fold isolates the fold rule.
        let never_folds = EdgePolicy {
            allowed: SIDES,
            foldable: false,
            resize: Resize::Fixed,
        };
        assert!(
            never_folds
                .admit_opening(EdgePlacement::open(ChromeEdge::Left, 230))
                .is_ok(),
            "opening unfolded is fine"
        );
        let refused = never_folds
            .admit_opening(EdgePlacement::folded_at(ChromeEdge::Left, 230))
            .expect_err("it declared it does not fold");
        assert!(matches!(refused, EdgeRefusal::NotFoldable), "{refused:?}");
    }

    #[test]
    fn r1902_a_panel_may_not_open_on_an_edge_its_own_declaration_excludes() {
        let policy = EdgePolicy::movable(&[ChromeEdge::Left]);
        let refused = policy
            .admit_opening(EdgePlacement::open(ChromeEdge::Right, 230))
            .expect_err("right is not among the allowed edges");
        let EdgeRefusal::EdgeNotAllowed { asked, allowed } = refused else {
            panic!("{refused:?}")
        };
        assert_eq!(asked, ChromeEdge::Right);
        assert_eq!(allowed, &[ChromeEdge::Left]);
    }

    /// ★★★★★ A `Fixed` panel's extent is checked against NOTHING, and that is
    /// the absence of a predicate rather than an exemption from one — routing
    /// it through `admit_extent` would refuse every fixed panel for having any
    /// extent at all.
    #[test]
    fn r1902_a_fixed_width_panel_opens_at_its_own_width() {
        let policy = EdgePolicy::movable(SIDES);
        assert!(matches!(policy.resize, Resize::Fixed));
        for extent in [1_u32, 54, 230, 9_999] {
            assert!(
                policy
                    .admit_opening(EdgePlacement::open(ChromeEdge::Left, extent))
                    .is_ok(),
                "a fixed panel declares no range, so {extent} violates nothing"
            );
        }
    }

    #[test]
    fn r1902_a_resizable_panel_may_not_open_outside_its_own_range() {
        let policy = EdgePolicy {
            allowed: SIDES,
            foldable: true,
            resize: Resize::Between { min: 180, max: 420 },
        };
        assert!(
            policy
                .admit_opening(EdgePlacement::open(ChromeEdge::Left, 180))
                .is_ok(),
            "the floor of the range is inside it"
        );
        let refused = policy
            .admit_opening(EdgePlacement::open(ChromeEdge::Left, 120))
            .expect_err("120 is below the declared floor");
        let EdgeRefusal::ExtentOutOfRange { asked, min, max } = refused else {
            panic!("{refused:?}")
        };
        assert_eq!((asked, min, max), (120, 180, 420));
    }

    /// ★★★★★ **The opening is judged by the same rules a gesture is** — one
    /// rule, two moments. Asserted as an equality over a grid rather than as a
    /// sentence, because "the same rules" is exactly the claim a second
    /// spelling would quietly break.
    #[test]
    fn r1902_opening_folded_and_folding_after_the_fact_agree_everywhere() {
        for foldable in [false, true] {
            for edge in [ChromeEdge::Left, ChromeEdge::Right, ChromeEdge::Top] {
                let policy = EdgePolicy {
                    allowed: SIDES,
                    foldable,
                    resize: Resize::Fixed,
                };
                let opened = policy.admit_opening(EdgePlacement::folded_at(edge, 230));
                // The same end state reached the other way: open there, then fold.
                let gestured = policy
                    .admit(EdgePlacement::open(ChromeEdge::Left, 230), edge)
                    .and_then(|at| policy.admit_fold(at, true));
                assert_eq!(
                    opened.is_ok(),
                    gestured.is_ok(),
                    "foldable={foldable} edge={edge:?}: a state reachable by gesture \
                     must be declarable as an opening, and one that is refused as a \
                     gesture must be refused as an opening"
                );
                if let (Ok(a), Ok(b)) = (opened, gestured) {
                    assert_eq!(a, b, "and they are the same placement");
                }
            }
        }
    }
}
