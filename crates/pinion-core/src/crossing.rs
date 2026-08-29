//! R1898 §5.16 §5.49 §2 #2 — **a drag that crosses a container's edge**, as ONE
//! value the preview and the release both read.
//!
//! # The fork this closes
//!
//! A board and the loose space around it are two homes for one unit. Before
//! this module a drag could only end in the home it began in: measured on the
//! assembled analysis tool at R1898, a card gripped on the board and carried
//! off it answered [`Dropped::Abandoned`](crate::widgets::tile_grid::Dropped::Abandoned)
//! — "nothing happened, and nothing was wrong" — and a detached panel dragged
//! back over the board slid across it and came to rest on top of it. Both
//! crossings existed as *controls* (a tear-off mark, a re-dock mark) and
//! neither existed as a *gesture*, which is the asymmetry this closes.
//!
//! ⇒ ★★★★★ *An edge a drag can cross needs a value saying which side a release
//! would land on, or the preview and the release are two computations that
//! agree until one of them is edited.* R1668 measured that exact cost one layer
//! down, on a board's own move gesture, and R1733's
//! [`TileDrag`](crate::widgets::tile_grid::TileDrag) answered it for a drag
//! that stays inside. This answers it for one that does not.
//!
//! # Why four passages and no fifth
//!
//! Where a drag *began* and where it *rests* are each one of two sides, so what
//! a release means is one of exactly four things ([`Passage`]). Writing them as
//! an enum rather than as two booleans a caller compares is what makes "it left
//! AND it joined" unspellable — the R1891 argument
//! ([`DetachHome`](crate::detach::DetachHome)), one axis over.
//!
//! # ★★★★★ Why the policy is one bit and not two
//!
//! The first draft of this module had a three-armed policy: cross both ways,
//! join only, neither. Asking rule (5) of it — *is there a path on which this
//! value differs from its neighbour* — retired an arm before it shipped. A
//! unit's reachable direction is settled by [`Side`], not by any declaration:
//! a drag that began [`Inside`](Side::Inside) can only ever *leave*, and one
//! that began [`Outside`](Side::Outside) can only ever *join*. So a
//! "may join, may not leave" policy differs from "may do either" on no input
//! at all, and a gate could never tell them apart.
//!
//! What is left is a single real question — *is this gesture about where the
//! unit lives?* — and it is genuinely two-valued, because dragging a loose
//! panel around, and sizing it by its corner, are gestures that must NOT dock
//! it when the pointer passes over the container. [`CrossingPolicy::Stays`]
//! carries the sentence saying so, so the refusal a person meets names the
//! gesture that would have worked instead of only saying no.
//!
//! # What this module does NOT decide
//!
//! *Where* the cell is, and *where* the point is. The container owns the first
//! (a grid resolves a pointer to a landing its own footprint rules clamp) and
//! the host owns the second. This module takes both as given and says what
//! crossing between them means, so a host cannot spell the question two ways.
//!
//! # Where the floor stands, measured
//!
//! Read from the floor toolkit's own 6.11.1 sources at R1898, over the four
//! files that implement its detachable panels:
//!
//! * Everything its detachable-panel class publishes about *where a panel is*
//!   answers one of four questions — is it loose (a bool), which region holds
//!   it, which regions it may be in, is this region allowed — and it emits
//!   **five** change signals, one of which is the region changing. **None of
//!   them answers where a release, right now, would put it.**
//!
//!   ⚠ Not a member count. R1891 measured that class through its runtime
//!   meta-object and recorded 104 published members
//!   ([`crate::detach`]); this paragraph reads its public header, which is a
//!   different population, and quoting a second number beside the first would
//!   read as a contradiction rather than as a second measurement.
//! * The words a prospective placement would have to be named with — would,
//!   prospective, preview, candidate, proposed, pending — occur **zero** times
//!   across those four files, private headers included. There is no such value
//!   to publish.
//! * It has this module's one bit, and keeps it private: a flag on the drag
//!   state, set from a held modifier key, that three sites at drop time branch
//!   on to skip the dock. It lives in a private header, it carries no sentence,
//!   and nothing publishes it — so a reader there can hold the key and watch,
//!   and cannot ask what letting go would do.
//! * Its refusal is a bool asked *before* a drag. R1801 measured the companion
//!   fact on the running floor: a region a panel declared it does not admit
//!   still accepts an imperative move, silently.
//!
//! So the floor answers "is it out?" and this module answers "what would
//! letting go, here, do — and if the answer is nothing, why".

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::external::RefusalReason;
use crate::input::DragLatch;

/// Which side of a container's edge something is on.
///
/// Two arms and no third: a unit is in the container or it is not, and a
/// "somewhere in between" would be a state no release could resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    /// In the container.
    Inside,
    /// Out of it, in the surrounding surface.
    Outside,
}

impl Side {
    /// The wire word, so a client can branch without parsing a sentence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inside => "inside",
            Self::Outside => "outside",
        }
    }
}

/// Where a release would put what a drag is carrying — **in the units that side
/// speaks**.
///
/// The two sides do not share a coordinate system, and pretending they do is
/// the defect this shape forecloses: a container places by cell and the surface
/// around it places by pixel, so a single `(x, y)` here would have two meanings
/// and every reader would have to remember which one it was holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rest {
    /// In the container, at this cell — already resolved through whatever
    /// footprint rules the container has, so it is a landing and not a guess.
    Inside {
        /// The column.
        col: u32,
        /// The row.
        row: u32,
    },
    /// Out of it, at this point in the surrounding surface's own space.
    Outside {
        /// The horizontal position.
        x: u32,
        /// The vertical position.
        y: u32,
    },
}

impl Rest {
    /// Which side this rest is on.
    #[must_use]
    pub const fn side(self) -> Side {
        match self {
            Self::Inside { .. } => Side::Inside,
            Self::Outside { .. } => Side::Outside,
        }
    }

    /// A cell in the container.
    #[must_use]
    pub const fn cell(col: u32, row: u32) -> Self {
        Self::Inside { col, row }
    }

    /// A point outside it.
    #[must_use]
    pub const fn point(x: u32, y: u32) -> Self {
        Self::Outside { x, y }
    }
}

/// Whether this gesture is about where the unit lives.
///
/// # Why an enum with two arms and no catch-all
///
/// Because [`Stays`](Self::Stays) has to be a CLAIM rather than a gap. A
/// gesture that carries a unit over the container's edge without meaning to put
/// it there is real and common — moving a loose panel across the board it came
/// off, and sizing that panel by its corner, are both — and the honest way to
/// say so is an arm that says it, with the reason attached. What must not exist
/// is a way to arrive here having said *nothing*, because then "the author did
/// not think about it" and "this gesture does not move the unit" would be one
/// value and no gate could tell them apart. There is no [`Default`], so every
/// gesture has answered.
///
/// See the module header for why the arm that used to sit between these two was
/// removed before it shipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrossingPolicy {
    /// A release on the other side of the edge moves the unit there.
    Crosses,
    /// It does not — and this is why, in the words a person meets.
    ///
    /// The sentence is required rather than optional, because *no* is not
    /// actionable and *no, drag its re-dock mark instead* is. R1706's rule.
    Stays(Cow<'static, str>),
}

impl CrossingPolicy {
    /// A gesture that does not move the unit across the edge, and why.
    #[must_use]
    pub fn stays(because: impl Into<Cow<'static, str>>) -> Self {
        Self::Stays(because.into())
    }

    /// Whether a release on the other side would move the unit there.
    #[must_use]
    pub const fn crosses(&self) -> bool {
        matches!(self, Self::Crosses)
    }

    /// Why not, for a policy that does not cross.
    #[must_use]
    pub fn because(&self) -> Option<&str> {
        match self {
            Self::Crosses => None,
            Self::Stays(why) => Some(why),
        }
    }

    /// The wire word.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Crosses => "crosses",
            Self::Stays(_) => "stays",
        }
    }
}

/// What a release does, given where the drag began and where it rests.
///
/// Exactly the four combinations of two sides, so a caller's `match` is total
/// and the two that CHANGE which side the unit is on are named rather than
/// inferred from a comparison somebody has to remember to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Passage {
    /// Began in the container and rests in it: a move within.
    Moved {
        /// The column it moves to.
        col: u32,
        /// The row it moves to.
        row: u32,
    },
    /// Began in the container and rests outside: the unit leaves.
    Left {
        /// Where it comes to rest, in the surrounding surface's space.
        x: u32,
        /// Likewise.
        y: u32,
    },
    /// Began outside and rests in the container: the unit joins.
    Joined {
        /// The column it joins at.
        col: u32,
        /// The row it joins at.
        row: u32,
    },
    /// Began outside and rests outside: a move in the surrounding surface.
    Drifted {
        /// Where it comes to rest.
        x: u32,
        /// Likewise.
        y: u32,
    },
}

impl Passage {
    /// Whether this passage changes which side of the edge the unit is on.
    ///
    /// The one question a host asks before touching two models instead of one,
    /// and a derivation rather than a fifth arm — see the module header.
    #[must_use]
    pub const fn crosses(self) -> bool {
        matches!(self, Self::Left { .. } | Self::Joined { .. })
    }

    /// The wire word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Moved { .. } => "moved",
            Self::Left { .. } => "left",
            Self::Joined { .. } => "joined",
            Self::Drifted { .. } => "drifted",
        }
    }
}

/// Why a crossing was refused.
///
/// ★ Carries the unit's name AND the policy's sentence, because a refusal a
/// person cannot act on is a refusal that gets worked around — this tree's
/// standing rule since R1706. Which direction was refused is in the arm, so a
/// client can branch without reading the words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrossingRefusal {
    /// The drag would take the unit out, and this gesture does not do that.
    MayNotLeave {
        /// Which unit.
        unit: String,
        /// The policy's sentence.
        because: Cow<'static, str>,
    },
    /// The drag would put the unit in, and this gesture does not do that.
    MayNotJoin {
        /// Which unit.
        unit: String,
        /// The policy's sentence.
        because: Cow<'static, str>,
    },
}

impl CrossingRefusal {
    /// The sentence, in the vocabulary an `External` refusal already uses.
    #[must_use]
    pub fn reason(&self) -> RefusalReason {
        match self {
            Self::MayNotLeave { unit, because } => RefusalReason::from(format!(
                "dragging does not take {unit} out of here: {because}"
            )),
            Self::MayNotJoin { unit, because } => {
                RefusalReason::from(format!("dragging does not put {unit} in here: {because}"))
            }
        }
    }

    /// A short machine word for the wire, so a client can branch without
    /// parsing the sentence.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::MayNotLeave { .. } => "may-not-leave",
            Self::MayNotJoin { .. } => "may-not-join",
        }
    }

    /// Which unit the refusal is about.
    #[must_use]
    pub fn unit(&self) -> &str {
        match self {
            Self::MayNotLeave { unit, .. } | Self::MayNotJoin { unit, .. } => unit,
        }
    }

    /// The policy's own sentence, without the framing.
    #[must_use]
    pub fn because(&self) -> &str {
        match self {
            Self::MayNotLeave { because, .. } | Self::MayNotJoin { because, .. } => because,
        }
    }
}

/// A drag that may cross a container's edge, in flight.
///
/// # The one fact the preview and the release share
///
/// [`passage`](Self::passage) is read by whatever draws the drag's destination
/// AND by whatever commits the release, so the two cannot disagree about which
/// side the unit is landing on. That is the discipline
/// [`TileDrag`](crate::widgets::tile_grid::TileDrag) applies to the cell within
/// a container, applied to the edge around it.
///
/// # Examples
///
/// ```
/// use pinion_core::crossing::{Crossing, CrossingPolicy, Passage, Rest, Side};
///
/// // A card gripped on a board at (2, 0), carried off it.
/// let mut drag = Crossing::open(
///     "message stream",
///     CrossingPolicy::Crosses,
///     Side::Inside,
///     (300.0, 100.0),
///     Rest::cell(2, 0),
/// );
/// assert_eq!(drag.passage(), Ok(Passage::Moved { col: 2, row: 0 }));
///
/// // A press that has not travelled is still a click: the rest does not move.
/// drag.hover((302.0, 101.0), Rest::point(900, 120));
/// assert!(!drag.is_drag());
/// assert_eq!(drag.passage(), Ok(Passage::Moved { col: 2, row: 0 }));
///
/// drag.hover((900.0, 120.0), Rest::point(900, 120));
/// assert!(drag.is_drag());
/// assert_eq!(drag.passage(), Ok(Passage::Left { x: 900, y: 120 }));
/// assert!(drag.crosses());
///
/// // Moving a loose panel is a gesture about position, not about where the
/// // panel lives — so carrying it across the edge docks nothing, and the
/// // refusal names the gesture that would have.
/// let mut moving = Crossing::open(
///     "message stream",
///     CrossingPolicy::stays("drag its re-dock mark to put it back on the board"),
///     Side::Outside,
///     (900.0, 120.0),
///     Rest::point(900, 120),
/// );
/// moving.hover((300.0, 100.0), Rest::cell(2, 0));
/// let refused = moving.passage().unwrap_err();
/// assert_eq!(refused.wire_word(), "may-not-join");
/// assert!(refused.reason().as_str().contains("re-dock mark"));
/// assert!(!moving.crosses());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crossing {
    /// What is being dragged, for the refusal to name.
    unit: String,
    /// Whether this gesture is about where the unit lives.
    policy: CrossingPolicy,
    /// Which side it was on when the drag opened.
    began: Side,
    /// ★★★★★ R1898 — whether the press has become a drag at all.
    ///
    /// The framework's own latch over its own threshold, not a rule spelled
    /// again here: a press and a release in the same place is a CLICK, and a
    /// click does not move a unit across an edge.
    latch: DragLatch,
    /// Where a release would put it, as last reported.
    rest: Rest,
}

impl Crossing {
    /// Open a crossing for `unit`, which began on `began`, at the pointer
    /// position `from`, with the unit resting at `at`.
    ///
    /// `at` is where the unit ALREADY is, expressed on the side it is on — the
    /// cell it occupies, or a point outside. Not the pointer's classification:
    /// at the instant of a press a release would move nothing, and a crossing
    /// that opened by saying otherwise would report a passage for a gesture
    /// that has not happened.
    ///
    /// ★★★★★ Measured on the running application before this was so: pressing
    /// a detached panel's re-dock mark and letting go without moving DOCKED the
    /// panel — at column 6, displacing five cards — because the mark sits over
    /// the board and the opening rest was read off the pointer. The in-process
    /// gate did not see it, because that driver delivers one fewer cursor event
    /// for the same gesture. ⇒ *a rule that depends on how many pointer events
    /// a driver sends is not a rule.*
    #[must_use]
    pub fn open(
        unit: impl Into<String>,
        policy: CrossingPolicy,
        began: Side,
        from: (f64, f64),
        at: Rest,
    ) -> Self {
        Self {
            unit: unit.into(),
            policy,
            began,
            latch: DragLatch::new(from),
            rest: at,
        }
    }

    /// Report the pointer at `cursor`, and where a release would then put the
    /// unit.
    ///
    /// Until the press strays past the framework's click-vs-drag threshold the
    /// gesture is still a click, and the rest does not move — so a control that
    /// happens to sit over the other side keeps doing what pressing it has
    /// always done.
    pub fn hover(&mut self, cursor: (f64, f64), at: Rest) {
        if self.latch.advance(cursor) {
            self.rest = at;
        }
    }

    /// Whether this press has become a drag.
    #[must_use]
    pub const fn is_drag(&self) -> bool {
        self.latch.live()
    }

    /// What is being dragged.
    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// Whether this gesture is about where the unit lives.
    #[must_use]
    pub const fn policy(&self) -> &CrossingPolicy {
        &self.policy
    }

    /// Which side the drag began on.
    #[must_use]
    pub const fn began(&self) -> Side {
        self.began
    }

    /// Where a release would put it.
    #[must_use]
    pub const fn rest(&self) -> Rest {
        self.rest
    }

    /// What a release would do — or why it would be refused.
    ///
    /// # Errors
    ///
    /// [`CrossingRefusal::MayNotLeave`] / [`CrossingRefusal::MayNotJoin`] when
    /// the passage would cross an edge this gesture does not cross. The two
    /// passages that stay on one side are never refused here: whether a
    /// container takes a move is the container's answer, not the edge's.
    pub fn passage(&self) -> Result<Passage, CrossingRefusal> {
        let because = || match &self.policy {
            CrossingPolicy::Crosses => Cow::Borrowed(""),
            CrossingPolicy::Stays(why) => why.clone(),
        };
        match (self.began, self.rest) {
            (Side::Inside, Rest::Inside { col, row }) => Ok(Passage::Moved { col, row }),
            (Side::Outside, Rest::Outside { x, y }) => Ok(Passage::Drifted { x, y }),
            (Side::Inside, Rest::Outside { x, y }) => {
                if self.policy.crosses() {
                    Ok(Passage::Left { x, y })
                } else {
                    Err(CrossingRefusal::MayNotLeave {
                        unit: self.unit.clone(),
                        because: because(),
                    })
                }
            }
            (Side::Outside, Rest::Inside { col, row }) => {
                if self.policy.crosses() {
                    Ok(Passage::Joined { col, row })
                } else {
                    Err(CrossingRefusal::MayNotJoin {
                        unit: self.unit.clone(),
                        because: because(),
                    })
                }
            }
        }
    }

    /// Whether a release right now would move the unit to the other side.
    ///
    /// `false` for a refused crossing, which is the point: a preview that drew
    /// a destination the release will not honour is the lie this whole module
    /// exists to make unspellable.
    #[must_use]
    pub fn crosses(&self) -> bool {
        self.passage().is_ok_and(Passage::crosses)
    }
}

#[cfg(test)]
mod tests {
    use super::{Crossing, CrossingPolicy, Passage, Rest, Side};

    /// Every combination of two sides answers, and each answers differently.
    #[test]
    fn the_four_passages_are_the_four_combinations_of_two_sides() {
        let cases = [
            (
                Side::Inside,
                Rest::cell(1, 2),
                Passage::Moved { col: 1, row: 2 },
            ),
            (
                Side::Inside,
                Rest::point(30, 40),
                Passage::Left { x: 30, y: 40 },
            ),
            (
                Side::Outside,
                Rest::cell(1, 2),
                Passage::Joined { col: 1, row: 2 },
            ),
            (
                Side::Outside,
                Rest::point(30, 40),
                Passage::Drifted { x: 30, y: 40 },
            ),
        ];
        let mut seen = Vec::new();
        for (began, rest, want) in cases {
            let drag = Crossing::open("unit", CrossingPolicy::Crosses, began, (0.0, 0.0), rest);
            assert_eq!(drag.passage(), Ok(want), "began {began:?} resting {rest:?}");
            assert_eq!(rest.side(), drag.rest().side());
            seen.push(want.as_str());
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 4, "the four cases must not collapse: {seen:?}");
    }

    /// Only the two that change sides are crossings.
    #[test]
    fn a_passage_that_stays_on_one_side_is_not_a_crossing() {
        assert!(!Passage::Moved { col: 0, row: 0 }.crosses());
        assert!(!Passage::Drifted { x: 0, y: 0 }.crosses());
        assert!(Passage::Left { x: 0, y: 0 }.crosses());
        assert!(Passage::Joined { col: 0, row: 0 }.crosses());
    }

    /// ★ Both directions of refusal are reachable, and each sentence carries
    /// the policy's own words — the half that makes a refusal actionable.
    #[test]
    fn a_refusal_names_the_unit_the_direction_and_the_way_that_works() {
        let mut moving = Crossing::open(
            "message stream",
            CrossingPolicy::stays("drag its re-dock mark to put it back on the board"),
            Side::Outside,
            (1.0, 1.0),
            Rest::point(1, 1),
        );
        moving.hover((400.0, 300.0), Rest::cell(0, 0));
        let refused = moving.passage().expect_err("this gesture does not dock");
        assert_eq!(refused.wire_word(), "may-not-join");
        assert_eq!(refused.unit(), "message stream");
        assert_eq!(
            refused.because(),
            "drag its re-dock mark to put it back on the board"
        );
        let sentence = refused.reason().as_str().to_owned();
        for half in ["message stream", "re-dock mark"] {
            assert!(
                sentence.contains(half),
                "the sentence must carry {half:?}: {sentence}"
            );
        }
        assert!(!moving.crosses(), "a refused crossing is not a crossing");

        let mut pinned = Crossing::open(
            "latency",
            CrossingPolicy::stays("a maximised card belongs to the arrangement it restores into"),
            Side::Inside,
            (0.0, 0.0),
            Rest::cell(0, 0),
        );
        assert_eq!(pinned.passage(), Ok(Passage::Moved { col: 0, row: 0 }));
        pinned.hover((500.0, 500.0), Rest::point(5, 5));
        let refused = pinned.passage().expect_err("this gesture does not detach");
        assert_eq!(refused.wire_word(), "may-not-leave");
        assert!(refused.reason().as_str().contains("maximised"));
        assert_eq!(pinned.unit(), "latency");
        assert_eq!(pinned.began(), Side::Inside);
    }

    /// ★★★★★ A press that never travelled is a click, and a click crosses
    /// nothing — however far over the other side the control it hit happens to
    /// sit.
    ///
    /// The property the running application taught this module: a detached
    /// panel's re-dock mark is painted OVER the board, so a press on it reports
    /// a cell, and reading the rest off the pointer at that moment docked the
    /// panel on a plain click. The threshold is
    /// [`DRAG_CLICK_THRESHOLD_PX`](crate::input::DRAG_CLICK_THRESHOLD_PX), so
    /// this rule and every other click-vs-drag rule in the tree cannot disagree.
    #[test]
    fn a_press_that_never_travelled_crosses_nothing() {
        let press = (400.0, 300.0);
        let mut click = Crossing::open(
            "message stream",
            CrossingPolicy::Crosses,
            Side::Outside,
            press,
            Rest::point(400, 300),
        );
        // Every point inside the threshold, reported as a cell — which is what
        // a control painted over the container answers.
        for jitter in [(0.0, 0.0), (1.0, 1.0), (3.0, 0.0), (0.0, -3.0)] {
            click.hover((press.0 + jitter.0, press.1 + jitter.1), Rest::cell(6, 0));
            assert!(!click.is_drag(), "{jitter:?} is inside the threshold");
            assert_eq!(
                click.passage(),
                Ok(Passage::Drifted { x: 400, y: 300 }),
                "a click leaves the unit where it is, {jitter:?}"
            );
            assert!(!click.crosses());
        }
        // And one that is outside it does become a drag, so this is not a test
        // of a latch that never fires.
        click.hover((press.0 + 40.0, press.1), Rest::cell(6, 0));
        assert!(click.is_drag());
        assert_eq!(click.passage(), Ok(Passage::Joined { col: 6, row: 0 }));
        // W3C, and `DragLatch`'s own rule: once a drag, always a drag.
        click.hover(press, Rest::cell(1, 1));
        assert!(click.is_drag());
        assert_eq!(click.passage(), Ok(Passage::Joined { col: 1, row: 1 }));
    }

    /// The policy answers the one question it exists for, both ways, and a
    /// crossing policy carries no sentence to mislead a caller with.
    #[test]
    fn a_policy_that_crosses_has_nothing_to_explain() {
        assert!(CrossingPolicy::Crosses.crosses());
        assert_eq!(CrossingPolicy::Crosses.because(), None);
        assert_eq!(CrossingPolicy::Crosses.as_str(), "crosses");

        let stays = CrossingPolicy::stays("because");
        assert!(!stays.crosses());
        assert_eq!(stays.because(), Some("because"));
        assert_eq!(stays.as_str(), "stays");
    }

    /// A crossing survives the round trip a `Signal` requires of it.
    #[test]
    fn a_crossing_in_flight_round_trips_through_its_wire_form() {
        let drag = Crossing::open(
            "message stream",
            CrossingPolicy::Crosses,
            Side::Inside,
            (0.0, 0.0),
            Rest::point(12, 34),
        );
        let json = serde_json::to_string(&drag).expect("a crossing serialises");
        let back: Crossing = serde_json::from_str(&json).expect("and comes back");
        assert_eq!(back, drag);
        assert_eq!(back.policy(), &CrossingPolicy::Crosses);
        assert_eq!(back.passage(), Ok(Passage::Left { x: 12, y: 34 }));
        assert_eq!(Side::Inside.as_str(), "inside");
        assert_eq!(Side::Outside.as_str(), "outside");
    }
}
