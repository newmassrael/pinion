//! R1900 §5.49 §2 #7 — **one place several units share, with exactly one of
//! them in front.**
//!
//! ⚠ The citation is `workspace`'s, not `tile_grid`'s, and that is deliberate:
//! §5.21 is the LAYOUT system, and the note below on what this module does not
//! decide is exactly the reason it would be the wrong section to name. This is
//! a model that gets published — the same shape [`crate::workspace`] is.
//!
//! # The distinction that was missing
//!
//! A board's cell and the card in it were one name. `Tile.id` *was* the card's
//! id, so the arrangement could say where a card is and could not say that two
//! cards are in the same place: the type had no room for the second one. That
//! is the same shape R1893 met one axis over — a delete could not exist because
//! *where an arrangement came from* was not a distinction the map carried — and
//! it has the same consequence. ⇒ ★★★★★ *A capability that is absent may be
//! the downstream of a distinction that is absent.*
//!
//! [`Stack`] is that distinction as a value: an ordered, **never empty** set of
//! units and the index of the one a reader sees.
//!
//! # Why the emptiness is the type's problem and not the caller's
//!
//! A place holding nothing is not a place — a board would draw an empty
//! rectangle nobody can reach and no gesture can fill. So there is no
//! constructor that makes an empty one ([`Stack::of`] takes the sole member by
//! value), and [`Stack::part`] refuses to remove the last one by name rather
//! than leaving the caller to check first. The same argument settles `fore`:
//! it is an index this module keeps in range, not a public field a caller can
//! set past the end.
//!
//! The floor toolkit's dock area spells the pair as two lists and an implicit
//! current, and R1900 measured what that costs at its own API surface — see
//! the table on [`Stack`].
//!
//! # What this module does NOT decide
//!
//! *Where* the place is, *how big* it is, and *what a tab strip looks like*.
//! A grid owns the first two ([`crate::widgets::tile_grid`]) and a painter owns
//! the third. This module answers only who shares a place and who is in front,
//! so those three cannot each keep their own answer.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::external::RefusalReason;

/// A place's occupants: ordered, never empty, exactly one in front.
///
/// # Against the floor toolkit at 6.11.1
///
/// Read from its own sources at R1900, over the class that stacks detachable
/// panels into one region and the window class that owns the operation.
///
/// | question | there | here |
/// |---|---|---|
/// | stacking two panels together | a `void` call, so there is **nothing to refuse with** — stacking a panel onto itself, or onto one it already shares with, is spelled the same as a legal one | [`Stack::join`], and a refusal that names the unit |
/// | which panel is in front | **no accessor publishes it.** The set is readable (one call answers the panels sharing a region) and the front one is only observable indirectly, through an activation signal after the fact | [`Stack::fore`] |
/// | taking one back out | no verb. A panel leaves a stack by being *added somewhere else*, so "un-stack" is a side effect of another operation rather than an act with an outcome | [`Stack::part`], returning what left and who took its place |
/// | the last one leaving | nothing forbids it; the region simply stops existing | [`StackRefusal::Sole`], which names the gesture that works instead |
///
/// ⚠ Its region *does* publish which edge the tab strip sits on, per area, and
/// this module publishes nothing of the sort — that is a painter's question and
/// [`crate::widgets::tile_grid`]'s consumer answers it. The row is left out
/// rather than counted as a win.
///
/// # The stored form names the front rather than indexing it
///
/// ★★★★★ A place is stored as its occupants plus **the one in front, by
/// name** ([`Occupants`]) — never as an index. An index can be past the end,
/// so a derived `Deserialize` would admit a stored place this type's own
/// methods panic on; a name can only be absent, which is one defect instead of
/// two and the one a person reading the file can see. [`Stack::rebuild`] is
/// that check, and it is public because a consumer with its own wire form
/// ([`crate::widgets::tile_grid::Tile`] has one) must be able to reach the
/// same gate rather than re-deriving it.
///
/// # Examples
///
/// ```
/// use pinion_core::stacking::Stack;
///
/// let mut place = Stack::of("packet");
/// assert!(!place.is_shared(), "one occupant is not a stack");
/// assert_eq!(place.fore(), &"packet");
///
/// place.join("share").expect("a unit that is not here yet");
/// assert!(place.is_shared());
/// assert_eq!(place.fore(), &"share", "what a person just put here is what they see");
///
/// let revealed = place.reveal(&"packet").expect("a member");
/// assert_eq!((revealed.was, revealed.now), ("share", "packet"));
///
/// let parted = place.part(&"share").expect("not the last one");
/// assert_eq!(parted.left, "share");
/// assert_eq!(place.members(), &["packet"]);
///
/// // And the last one cannot leave: the place would hold nothing.
/// assert!(place.part(&"packet").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "Occupants<T>", try_from = "Occupants<T>")]
#[serde(bound(
    serialize = "T: Clone + PartialEq + fmt::Display + Serialize",
    deserialize = "T: Clone + PartialEq + fmt::Display + Deserialize<'de>"
))]
pub struct Stack<T> {
    /// In the order a tab strip draws them. Never empty — every mutation here
    /// either keeps that true or is refused.
    members: Vec<T>,
    /// Index into `members` of the one in front. Always in range.
    fore: usize,
}

/// A [`Stack`] as it is stored: who is here, and which of them is in front —
/// **by name**, for the reason on [`Stack`].
///
/// Public because it is the shape on the wire, and a client writing an
/// arrangement by hand is entitled to know it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occupants<T> {
    /// In the order a tab strip draws them.
    pub members: Vec<T>,
    /// The one a reader sees. Must be one of `members`.
    pub fore: T,
}

/// Why a stored place could not be read back.
///
/// Two arms, and that is the whole space: the stored form names its front
/// rather than indexing it, so "the index is past the end" cannot be said.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StackUnreadable {
    /// The stored place held nobody, and a place holds somebody.
    Empty,
    /// The stored front is not among the stored occupants.
    ForeAbsent {
        /// The front that was named.
        fore: String,
        /// Who was actually listed, in strip order.
        here: Vec<String>,
    },
}

impl StackUnreadable {
    /// The sentence, in the vocabulary an `External` refusal already uses.
    #[must_use]
    pub fn reason(&self) -> RefusalReason {
        match self {
            Self::Empty => {
                RefusalReason::from("a place with no occupants is not a place".to_owned())
            }
            Self::ForeAbsent { fore, here } => RefusalReason::from(format!(
                "{fore} is named as in front but is not here; {} is",
                here.join(", ")
            )),
        }
    }
}

impl fmt::Display for StackUnreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason().as_str())
    }
}

impl std::error::Error for StackUnreadable {}

impl<T> From<Stack<T>> for Occupants<T>
where
    T: Clone,
{
    fn from(stack: Stack<T>) -> Self {
        let fore = stack.members[stack.fore].clone();
        Self {
            members: stack.members,
            fore,
        }
    }
}

impl<T> TryFrom<Occupants<T>> for Stack<T>
where
    T: Clone + PartialEq + fmt::Display,
{
    type Error = StackUnreadable;

    fn try_from(stored: Occupants<T>) -> Result<Self, Self::Error> {
        Self::rebuild(stored.members, &stored.fore)
    }
}

/// What a [`Stack::reveal`] changed: who was in front, and who is now.
///
/// Both halves, because a caller repainting a place needs to know what stopped
/// being visible as much as what started — and a revealed unit that was already
/// in front is a legal, uninteresting outcome the two fields make readable
/// (`was == now`) rather than one an error would have to stand in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revealed<T> {
    /// Who was in front before.
    pub was: T,
    /// Who is in front now.
    pub now: T,
}

/// What a [`Stack::part`] removed, and who the place shows instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parted<T> {
    /// The unit that left.
    pub left: T,
    /// Who is in front of what remains. Always a member — the place is never
    /// emptied, so this is never absent.
    pub fore: T,
}

/// Why a stacking operation was refused, in words a person can act on.
///
/// Each arm names the unit, and [`StackRefusal::Sole`] additionally names the
/// gesture that *does* work — the R1706 rule that a refusal reaches a person
/// too, so it must say more than no.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StackRefusal {
    /// The unit is already one of this place's occupants.
    AlreadyHere {
        /// Which unit.
        unit: String,
    },
    /// The unit does not share this place, so it cannot be revealed or removed
    /// from it. The occupants are named, so a caller that asked with a stale id
    /// can see what the place actually holds.
    NotHere {
        /// Which unit was asked for.
        unit: String,
        /// Who is actually here, in strip order.
        here: Vec<String>,
    },
    /// The unit is the only occupant, so removing it would empty the place.
    Sole {
        /// Which unit.
        unit: String,
    },
}

impl StackRefusal {
    /// The sentence, in the vocabulary an `External` refusal already uses.
    #[must_use]
    pub fn reason(&self) -> RefusalReason {
        match self {
            Self::AlreadyHere { unit } => {
                RefusalReason::from(format!("{unit} already shares this place"))
            }
            Self::NotHere { unit, here } => RefusalReason::from(format!(
                "{unit} does not share this place; {} is here",
                here.join(", ")
            )),
            Self::Sole { unit } => RefusalReason::from(format!(
                "{unit} is the only one here and a place cannot be left empty; \
                 move the place itself instead"
            )),
        }
    }

    /// A short machine word for the wire, so a client can branch without
    /// parsing the sentence.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::AlreadyHere { .. } => "already-here",
            Self::NotHere { .. } => "not-here",
            Self::Sole { .. } => "sole",
        }
    }

    /// Which unit the refusal is about.
    #[must_use]
    pub fn unit(&self) -> &str {
        match self {
            Self::AlreadyHere { unit } | Self::NotHere { unit, .. } | Self::Sole { unit } => unit,
        }
    }
}

impl fmt::Display for StackRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason().as_str())
    }
}

impl std::error::Error for StackRefusal {}

impl<T> Stack<T>
where
    T: Clone + PartialEq + fmt::Display,
{
    /// A place with one occupant, which is therefore the one in front.
    #[must_use]
    pub fn of(sole: T) -> Self {
        Self {
            members: vec![sole],
            fore: 0,
        }
    }

    /// ★ A place read back from somewhere that promised nothing — a saved
    /// arrangement, a hand-written one, a client's `place` request.
    ///
    /// This is the one gate between a stored strip and a [`Stack`], so a
    /// consumer with its own wire form does not get to skip the check by
    /// spelling it differently.
    ///
    /// # Errors
    ///
    /// [`StackUnreadable::Empty`] when nobody was listed;
    /// [`StackUnreadable::ForeAbsent`] when the named front is not one of them.
    pub fn rebuild(members: Vec<T>, fore: &T) -> Result<Self, StackUnreadable> {
        if members.is_empty() {
            return Err(StackUnreadable::Empty);
        }
        let at =
            members
                .iter()
                .position(|m| m == fore)
                .ok_or_else(|| StackUnreadable::ForeAbsent {
                    fore: fore.to_string(),
                    here: members.iter().map(ToString::to_string).collect(),
                })?;
        Ok(Self { members, fore: at })
    }

    /// The occupants, in the order a tab strip draws them.
    #[must_use]
    pub fn members(&self) -> &[T] {
        &self.members
    }

    /// The one a reader sees.
    #[must_use]
    pub fn fore(&self) -> &T {
        &self.members[self.fore]
    }

    /// Where in the strip the front one sits.
    #[must_use]
    pub const fn fore_index(&self) -> usize {
        self.fore
    }

    /// How many share this place. Never zero.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Never true. Present because clippy asks for it beside [`Self::len`], and
    /// it is a claim worth being able to assert: a place holds someone.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether more than one unit shares this place — which is what makes a tab
    /// strip worth drawing, and the only condition a painter should branch on.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.members.len() > 1
    }

    /// The sole occupant, when there is exactly one.
    #[must_use]
    pub fn sole(&self) -> Option<&T> {
        (!self.is_shared()).then(|| &self.members[0])
    }

    /// Whether `unit` shares this place.
    #[must_use]
    pub fn holds(&self, unit: &T) -> bool {
        self.members.contains(unit)
    }

    /// Where `unit` sits in the strip.
    #[must_use]
    pub fn position(&self, unit: &T) -> Option<usize> {
        self.members.iter().position(|m| m == unit)
    }

    /// Add `unit` to this place and bring it to the front.
    ///
    /// It goes to the front because a person who just put it here is looking
    /// for it: a join that left the previous occupant showing would read as
    /// nothing having happened.
    ///
    /// # Errors
    ///
    /// [`StackRefusal::AlreadyHere`] when it is already an occupant.
    pub fn join(&mut self, unit: T) -> Result<(), StackRefusal> {
        if self.holds(&unit) {
            return Err(StackRefusal::AlreadyHere {
                unit: unit.to_string(),
            });
        }
        self.members.push(unit);
        self.fore = self.members.len() - 1;
        Ok(())
    }

    /// Add `unit` at `at` in the strip and bring it to the front.
    ///
    /// `at` beyond the end appends, the way a strip drop past the last tab
    /// means "last" rather than nothing.
    ///
    /// # Errors
    ///
    /// [`StackRefusal::AlreadyHere`] when it is already an occupant.
    pub fn join_at(&mut self, unit: T, at: usize) -> Result<(), StackRefusal> {
        if self.holds(&unit) {
            return Err(StackRefusal::AlreadyHere {
                unit: unit.to_string(),
            });
        }
        let at = at.min(self.members.len());
        self.members.insert(at, unit);
        self.fore = at;
        Ok(())
    }

    /// Bring `unit` to the front.
    ///
    /// # Errors
    ///
    /// [`StackRefusal::NotHere`] when it does not share this place.
    pub fn reveal(&mut self, unit: &T) -> Result<Revealed<T>, StackRefusal> {
        let at = self.position(unit).ok_or_else(|| self.not_here(unit))?;
        Ok(self.reveal_at_unchecked(at))
    }

    /// Bring the `at`th of the strip to the front.
    ///
    /// The index form exists because a tab strip's press knows *which tab* and
    /// not which unit, and converting in the caller is where a strip and a
    /// stack come to disagree about the order.
    ///
    /// # Errors
    ///
    /// [`StackRefusal::NotHere`] when `at` is past the end — with the occupants
    /// named, because a caller holding a stale index is holding a stale strip.
    pub fn reveal_at(&mut self, at: usize) -> Result<Revealed<T>, StackRefusal> {
        if at >= self.members.len() {
            return Err(StackRefusal::NotHere {
                unit: format!("tab {at}"),
                here: self.member_names(),
            });
        }
        Ok(self.reveal_at_unchecked(at))
    }

    /// Take `unit` out of this place.
    ///
    /// # Errors
    ///
    /// [`StackRefusal::NotHere`] when it does not share this place;
    /// [`StackRefusal::Sole`] when it is the only occupant — a place is never
    /// emptied, and the refusal names the gesture that moves the place itself.
    pub fn part(&mut self, unit: &T) -> Result<Parted<T>, StackRefusal> {
        let at = self.position(unit).ok_or_else(|| self.not_here(unit))?;
        if !self.is_shared() {
            return Err(StackRefusal::Sole {
                unit: unit.to_string(),
            });
        }
        let left = self.members.remove(at);
        // The front moves to the neighbour on the left when the front itself
        // left, and otherwise stays on the same unit — which is an index shift
        // when the departure was ahead of it in the strip.
        if self.fore > at || self.fore == self.members.len() {
            self.fore -= 1;
        }
        Ok(Parted {
            left,
            fore: self.fore().clone(),
        })
    }

    /// The occupants' names, in strip order — what a refusal reports and a wire
    /// publishes.
    #[must_use]
    pub fn member_names(&self) -> Vec<String> {
        self.members.iter().map(ToString::to_string).collect()
    }

    fn not_here(&self, unit: &T) -> StackRefusal {
        StackRefusal::NotHere {
            unit: unit.to_string(),
            here: self.member_names(),
        }
    }

    fn reveal_at_unchecked(&mut self, at: usize) -> Revealed<T> {
        let was = self.members[self.fore].clone();
        self.fore = at;
        Revealed {
            was,
            now: self.members[at].clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Stack, StackRefusal, StackUnreadable};

    #[test]
    fn a_stored_place_names_its_front_rather_than_indexing_it() {
        let mut place = Stack::of("one".to_owned());
        place.join("two".to_owned()).expect("not here yet");
        place.reveal(&"one".to_owned()).expect("a member");

        let wire = serde_json::to_string(&place).expect("a place is a value");
        assert!(
            wire.contains("\"fore\":\"one\""),
            "a reader sees who is in front, not a number: {wire}"
        );
        let back: Stack<String> = serde_json::from_str(&wire).expect("its own output");
        assert_eq!(back, place, "a place round trips");
    }

    #[test]
    fn a_stored_place_with_nobody_in_it_is_refused_rather_than_read() {
        let refused = serde_json::from_str::<Stack<String>>(r#"{"members":[],"fore":"gone"}"#)
            .expect_err("a place holds somebody");
        assert!(refused.to_string().contains("not a place"), "{refused}",);
    }

    #[test]
    fn a_stored_front_that_is_not_here_is_refused_and_the_refusal_lists_who_is() {
        let refused =
            serde_json::from_str::<Stack<String>>(r#"{"members":["one","two"],"fore":"three"}"#)
                .expect_err("the front must be an occupant");
        let sentence = refused.to_string();
        assert!(sentence.contains("three"), "{sentence}");
        assert!(sentence.contains("one, two"), "{sentence}");
    }

    #[test]
    fn rebuild_is_the_one_gate_and_it_names_both_defects() {
        assert_eq!(
            Stack::rebuild(Vec::<String>::new(), &"any".to_owned()),
            Err(StackUnreadable::Empty)
        );
        let refused = Stack::rebuild(vec!["a".to_owned()], &"b".to_owned())
            .expect_err("the front must be an occupant");
        assert!(matches!(refused, StackUnreadable::ForeAbsent { .. }));

        let built = Stack::rebuild(vec!["a".to_owned(), "b".to_owned()], &"b".to_owned())
            .expect("a listed front");
        assert_eq!(built.fore(), "b");
        assert_eq!(built.fore_index(), 1);
    }

    #[test]
    fn a_fresh_place_holds_its_sole_occupant_in_front() {
        let place = Stack::of("one");
        assert_eq!(place.len(), 1);
        assert!(!place.is_shared());
        assert_eq!(place.sole(), Some(&"one"));
        assert_eq!(place.fore(), &"one");
        assert_eq!(place.fore_index(), 0);
        assert!(!place.is_empty(), "a place holds somebody");
    }

    #[test]
    fn joining_brings_the_new_occupant_to_the_front() {
        let mut place = Stack::of("one");
        place.join("two").expect("not here yet");
        assert_eq!(place.members(), &["one", "two"]);
        assert_eq!(place.fore(), &"two");
        assert!(place.is_shared());
        assert_eq!(place.sole(), None);
    }

    #[test]
    fn joining_at_an_index_puts_it_there_and_past_the_end_appends() {
        let mut place = Stack::of("one");
        place.join_at("zero", 0).expect("not here yet");
        assert_eq!(place.members(), &["zero", "one"]);
        assert_eq!(place.fore(), &"zero");

        place.join_at("last", 99).expect("not here yet");
        assert_eq!(place.members(), &["zero", "one", "last"]);
        assert_eq!(place.fore(), &"last");
    }

    #[test]
    fn a_unit_cannot_join_a_place_it_already_shares() {
        let mut place = Stack::of("one");
        let refused = place.join("one").expect_err("already here");
        assert!(matches!(refused, StackRefusal::AlreadyHere { .. }));
        assert_eq!(refused.wire_word(), "already-here");
        assert_eq!(refused.unit(), "one");
        assert_eq!(place.len(), 1, "a refusal changes nothing");
    }

    #[test]
    fn revealing_reports_both_halves_and_a_no_op_is_readable() {
        let mut place = Stack::of("one");
        place.join("two").expect("not here yet");

        let moved = place.reveal(&"one").expect("a member");
        assert_eq!((moved.was, moved.now), ("two", "one"));

        let still = place.reveal(&"one").expect("a member");
        assert_eq!(
            still.was, still.now,
            "already in front is legal, not an error"
        );
    }

    #[test]
    fn revealing_by_index_is_the_strips_own_question() {
        let mut place = Stack::of("one");
        place.join("two").expect("not here yet");
        let moved = place.reveal_at(0).expect("in range");
        assert_eq!(moved.now, "one");

        let refused = place.reveal_at(7).expect_err("past the end");
        let StackRefusal::NotHere { here, .. } = &refused else {
            panic!("a stale index is a stale strip")
        };
        assert_eq!(here, &["one".to_owned(), "two".to_owned()]);
    }

    #[test]
    fn a_refusal_names_who_is_actually_here() {
        let mut place = Stack::of("one");
        place.join("two").expect("not here yet");
        let refused = place.reveal(&"absent").expect_err("not a member");
        assert_eq!(refused.wire_word(), "not-here");
        let sentence = refused.reason().as_str().to_owned();
        assert!(sentence.contains("absent"), "{sentence}");
        assert!(sentence.contains("one, two"), "{sentence}");
    }

    #[test]
    fn parting_names_what_left_and_who_took_its_place() {
        let mut place = Stack::of("one");
        place.join("two").expect("not here yet");
        place.join("three").expect("not here yet");
        assert_eq!(place.fore(), &"three");

        let gone = place.part(&"three").expect("not the last");
        assert_eq!(gone.left, "three");
        assert_eq!(
            gone.fore, "two",
            "the front falls back to its left neighbour"
        );
        assert_eq!(place.members(), &["one", "two"]);
    }

    #[test]
    fn parting_someone_ahead_of_the_front_keeps_the_front_on_the_same_unit() {
        let mut place = Stack::of("one");
        place.join("two").expect("not here yet");
        place.join("three").expect("not here yet");
        place.reveal(&"three").expect("a member");

        let gone = place.part(&"one").expect("not the last");
        assert_eq!(gone.fore, "three", "the same unit, at a shifted index");
        assert_eq!(place.fore_index(), 1);
    }

    #[test]
    fn the_last_occupant_cannot_leave_and_the_refusal_names_what_works() {
        let mut place = Stack::of("one");
        let refused = place.part(&"one").expect_err("a place is never emptied");
        assert!(matches!(refused, StackRefusal::Sole { .. }));
        assert_eq!(refused.wire_word(), "sole");
        let sentence = refused.reason().as_str().to_owned();
        assert!(sentence.contains("move the place itself"), "{sentence}");
        assert_eq!(place.len(), 1);
    }

    #[test]
    fn parting_a_unit_that_is_not_here_is_told_apart_from_parting_the_last_one() {
        let mut place = Stack::of("one");
        let refused = place.part(&"absent").expect_err("not a member");
        assert_eq!(
            refused.wire_word(),
            "not-here",
            "the sole check must not swallow the membership one"
        );
    }
}
