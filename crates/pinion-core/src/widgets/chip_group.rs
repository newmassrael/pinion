//! R1721 §5.38 §5.39 §5.40 — **a row of chips declares how many of its members
//! may be on, and everything else about it is derived from that one word.**
//!
//! ## The defect this exists for, measured by driving the screens
//!
//! Three screens of the analysis tool paint the same shape — a line of labelled
//! pills, each on or off — and until this round each of them decided, by hand
//! and separately, what that shape *is*. Measured on 2026-08-19 by driving the
//! running applications rather than by reading them:
//!
//! | screen | the row | what it declares | what it does |
//! |---|---|---|---|
//! | capture viewer | three saved filters | three independent `button`s, three Tab stops | **at most one**: choosing the second cleared the first, choosing it again cleared it |
//! | dashboard | five saved filters | five independent `button`s, **zero** Tab stops | **nothing at all** — a pointer press changed no state |
//! | chip gallery | four filters | four independent `button`s, four Tab stops | any subset |
//!
//! Two of the three announce a rule they do not obey. The capture viewer tells
//! a screen reader "toggle button, not pressed" three times over a set where
//! only one member can ever be on, and its own test file calls them
//! "independent switches". The dashboard's five are announced as operable
//! controls and are inert. Only the third row's declaration was true, and it
//! was true by luck rather than by construction: nothing anywhere related the
//! rule a row obeys to the way the row is announced and operated.
//!
//! ## The floor this is built to beat, measured rather than read
//!
//! A probe was built against the mature toolkit at 6.11.1 and **run** offscreen
//! — a row of three checkable buttons, joined by the one type that toolkit has
//! for expressing a selection rule over a set of buttons:
//!
//! * the rule does not reach the accessibility tree. An exclusive set and an
//!   independent set report the **same** member role, and it is the push-button
//!   role in both — a screen reader is never told the members are alternatives;
//! * the group is not a widget, so it has **no accessibility node at all**:
//!   nothing stands for the set, and its members are loose children of whatever
//!   encloses them;
//! * "at most one" is **not expressible**. Clicking the chosen member of an
//!   exclusive set leaves it chosen; the set cannot be emptied;
//! * the group publishes exactly two properties, one of which is its name. The
//!   rule is a bare boolean, and no roster, cursor, or key list can be read off
//!   it;
//! * ★ joining the set **costs the keyboard**: three loose checkable buttons are
//!   three Tab stops, and the moment they join a group — *even a non-exclusive
//!   one* — two of them accept neither `Tab` nor an arrow and are reachable
//!   only by pointer. Measured, both ways round;
//! * and `Home` / `End` move nothing inside the set.
//!
//! Everything this module derives is therefore a compile error there, and the
//! last two are defects rather than absences.
//!
//! ## What is derived, and from what
//!
//! [`Choice`] is the whole declaration. Six things follow from it and a screen
//! has nowhere to state any of them separately:
//!
//! | | [`Any`](Choice::Any) | [`AtMostOne`](Choice::AtMostOne) | [`ExactlyOne`](Choice::ExactlyOne) |
//! |---|---|---|---|
//! | what the group is | a `group` of toggle buttons | a single-select `listbox` | a `radiogroup` |
//! | what a member is | `button` + `aria-pressed` | `option` + `aria-selected` | `radio` + `aria-checked` |
//! | Tab stops | **one per member** | **one, for the row** | **one, for the row** |
//! | arrows | — (Tab moves between members) | move the cursor | move the cursor |
//! | arriving | — | only moves ([`Activation::Explicit`]) | also chooses ([`Activation::Follows`]) |
//! | choosing the member already on | turns it off | turns it off, leaving none | **refused** — it is the only one |
//!
//! **No two columns agree on all six**, which is what makes the derivation
//! observable: a test that changes only the word and re-reads the group can see
//! several of these move, and `r1721_the_three_rules_are_distinguishable_by_what_they_derive`
//! asserts the pairwise inequality rather than trusting this table. Individual
//! rows *do* agree — the two composite rules share their stop count and their
//! arrows, and `Any` and `AtMostOne` both turn off the chip that is on — and
//! saying otherwise here would be the R1720.1 class, a doc claiming more than
//! the code. The WAI-ARIA APG rule they encode is the ordinary one — a set whose
//! members are alternatives is a composite with a roving cursor, and a set of
//! independent switches is not — and the third column's refusal is what
//! "exactly one" *means* rather than a policy anybody chose.
//!
//! ## What this module is not
//!
//! It paints nothing and knows no accessibility type. The roles live in
//! `pinion_a11y::chip_group`, which is the only place they are spelled, and the
//! pill lives in `pinion_widget_paint::chip` (`chip_row` for a bar laid out by
//! its parent, `option_chip` for one chip). What is here is the rule, the
//! roster, what choosing does, and the [`Utterance`] that says so.

use crate::utterance::Utterance;
use crate::widgets::interaction::InteractionState;
use crate::widgets::roving::{Activation, Axis, Ends, Member, Roving, RovingSpec};

/// How many of a chip row's members may be on at once.
///
/// The one declaration a row makes. See the module documentation for the six
/// things derived from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Any subset, including none and all. The members are independent
    /// switches that happen to sit side by side, so each answers for itself and
    /// each is its own Tab stop.
    Any,
    /// At most one. Choosing a member replaces whichever was on; choosing the
    /// one that is on clears it, leaving none on.
    ///
    /// The rule a set of saved filters obeys: applying one replaces the last,
    /// and applying the current one again means "stop filtering".
    AtMostOne,
    /// Exactly one. Choosing a member replaces whichever was on; choosing the
    /// one that is on is **refused**, because clearing it would leave none and
    /// the row has said that cannot happen.
    ExactlyOne,
}

impl Choice {
    /// Every rule a row can declare, so a reader of this type can be walked
    /// over all of them rather than over the ones somebody remembered.
    pub const ALL: [Self; 3] = [Self::Any, Self::AtMostOne, Self::ExactlyOne];

    /// The rule's name, for a diagnostic and for a surface that wants to publish
    /// it.
    ///
    /// **One-way, deliberately.** The neighbouring vocabularies in
    /// [`roving`](crate::widgets::roving) carry a `wire` and no reader either,
    /// and R1719.1 deleted a `Fault::wire` built for symmetry with nothing on the
    /// other side of it. A rule is *declared in source* by the screen that owns
    /// the row — it is never received, because nothing outside the screen is
    /// entitled to change how many of its chips may be on. A `from_wire` would be
    /// a parser for a message that cannot arrive.
    ///
    /// The rule still reaches an agent, in a vocabulary it already knows: the
    /// group's ARIA role plus `aria-multiselectable` says it (a non-multiselect
    /// `listbox` is at most one, a `radiogroup` is exactly one, a `group` of
    /// `button[aria-pressed]` is any), which is why this is a *name* rather than
    /// the wire form of the fact.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::AtMostOne => "at-most-one",
            Self::ExactlyOne => "exactly-one",
        }
    }

    /// Whether the row is **one** Tab stop with a cursor inside it, rather than
    /// one stop per member.
    ///
    /// A set of alternatives is a WAI-ARIA composite; a set of independent
    /// switches is not. This is the sentence, and [`ChipGroup::stops`] is the
    /// list a screen's focus ring actually reads.
    #[must_use]
    pub const fn is_composite(self) -> bool {
        match self {
            Self::Any => false,
            Self::AtMostOne | Self::ExactlyOne => true,
        }
    }

    /// The cursor policy a composite row navigates by, or `None` when the row
    /// is not a composite and its members are reached by `Tab`.
    ///
    /// [`Axis::Both`] because a chip row wraps across lines when it is narrow,
    /// so neither axis alone is its reading order; [`Ends::Wrap`] because a set
    /// of peers has no last member (the [`Ends`] documentation names filter
    /// chips as the case); and the activation is the one thing that differs
    /// between the two composite rules — arriving at a radio chooses it,
    /// arriving at an option does not.
    #[must_use]
    pub const fn cursor(self) -> Option<RovingSpec> {
        match self {
            Self::Any => None,
            Self::AtMostOne => Some(
                RovingSpec::new(Axis::Both)
                    .with_ends(Ends::Wrap)
                    .with_activation(Activation::Explicit),
            ),
            Self::ExactlyOne => Some(
                RovingSpec::new(Axis::Both)
                    .with_ends(Ends::Wrap)
                    .with_activation(Activation::Follows),
            ),
        }
    }

    /// What choosing member `index` does to an on-set, given what is on now.
    ///
    /// Total and pure: the rule is applied here and nowhere else, so a screen
    /// cannot implement "at most one" by clearing the vector itself and then
    /// announce something that disagrees. An out-of-range index leaves the set
    /// alone and [`Outcome::Refused`] says so.
    #[must_use]
    pub fn apply(self, on: &[bool], index: usize) -> Outcome {
        let Some(&was_on) = on.get(index) else {
            return Outcome::Refused(Refusal::NoSuchMember);
        };
        match self {
            Self::Any => {
                let mut next = on.to_vec();
                next[index] = !was_on;
                Outcome::Set {
                    on: next,
                    now: !was_on,
                }
            }
            Self::AtMostOne => {
                let mut next = vec![false; on.len()];
                next[index] = !was_on;
                Outcome::Set {
                    on: next,
                    now: !was_on,
                }
            }
            Self::ExactlyOne if was_on => Outcome::Refused(Refusal::LastOneOn),
            Self::ExactlyOne => {
                let mut next = vec![false; on.len()];
                next[index] = true;
                Outcome::Set {
                    on: next,
                    now: true,
                }
            }
        }
    }
}

/// Why a choice was refused.
///
/// Two reasons, both of which are the rule speaking rather than a failure: a
/// row cannot be asked about a member it does not have, and an
/// [`ExactlyOne`](Choice::ExactlyOne) row cannot be emptied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The index is past the end of the roster.
    NoSuchMember,
    /// The member is the only one on and the row must keep one on.
    LastOneOn,
    /// The chip is [`ChipPosture::Locked`]. The cursor may rest on it and a
    /// reader is told it exists; choosing it is what refuses.
    Locked,
}

/// What [`Choice::apply`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The on-set after the choice, and whether the chosen member is now on.
    Set {
        /// The whole on-set, member for member.
        on: Vec<bool>,
        /// Whether the member that was chosen ended up on.
        now: bool,
    },
    /// Nothing changed, and this is why.
    Refused(Refusal),
}

/// A chip's interaction posture — where the pointer is, and whether the chip
/// may be chosen at all.
///
/// One value rather than a posture plus an `enabled` flag, because "locked" and
/// "at rest under a hovering pointer" are answers to the same question and two
/// names for one fact get one check between them (the R1716 rule). It is an
/// [`InteractionState`], so the paint's state-layer overlay and the
/// accessibility tree's `disabled` / `hovered` / `pressed` come from this one
/// declaration rather than from two that agree today.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChipPosture {
    /// At rest.
    #[default]
    Idle,
    /// The pointer is over the chip.
    Hover,
    /// The chip is held down.
    Pressed,
    /// Choosing the chip does nothing. It is still painted, still announced and
    /// still reachable by the cursor — the [`Member`] rule this row inherits,
    /// which is what makes a locked chip discoverable from a keyboard.
    Locked,
}

impl InteractionState for ChipPosture {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Locked)
    }
}

/// One chip: what it is called, where it is painted, and whether it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    /// The painted tag, which is also this chip's node in the accessibility
    /// tree and the cursor's `aria-activedescendant` while it rests here.
    pub tag: String,
    /// The chip's accessible name and the text on the pill.
    pub label: String,
    /// Whether the chip is on.
    pub on: bool,
    /// Where the pointer is, and whether the chip may be chosen.
    pub posture: ChipPosture,
}

impl Chip {
    /// A chip at rest that can be chosen.
    #[must_use]
    pub fn new(tag: impl Into<String>, label: impl Into<String>, on: bool) -> Self {
        Self {
            tag: tag.into(),
            label: label.into(),
            on,
            posture: ChipPosture::Idle,
        }
    }

    /// The same chip in a different posture.
    #[must_use]
    pub fn with_posture(mut self, posture: ChipPosture) -> Self {
        self.posture = posture;
        self
    }

    /// Whether choosing this chip does anything — derived from the posture, so
    /// a chip cannot be locked and enabled at once.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        !matches!(self.posture, ChipPosture::Locked)
    }
}

/// A row of chips and the rule it obeys.
///
/// Built per view from what the screen already holds — the labels and the
/// on-set — so it stores no state a screen would have to keep in step. Reading
/// it is how a screen learns what its own row is; [`choose`](Self::choose) is
/// how the row changes, and it is the only way, so the rule cannot be applied
/// in one place and announced from another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipGroup {
    tag: String,
    name: String,
    chips: Vec<Chip>,
    choice: Choice,
    cursor: Option<usize>,
}

impl ChipGroup {
    /// A row named `name`, painted at `tag`, obeying `choice`.
    ///
    /// The cursor starts on the chip that is on, or on the first chip when none
    /// is — [`with_cursor`](Self::with_cursor) is how a screen that has walked
    /// the cursor away from the chosen chip says so.
    #[must_use]
    pub fn new(
        tag: impl Into<String>,
        name: impl Into<String>,
        chips: Vec<Chip>,
        choice: Choice,
    ) -> Self {
        Self {
            tag: tag.into(),
            name: name.into(),
            chips,
            choice,
            cursor: None,
        }
    }

    /// Seat the cursor on chip `index`, overriding the default seat.
    ///
    /// An index past the end is ignored rather than clamped: a caller reporting
    /// a cursor the row does not have has a defect somewhere else, and silently
    /// moving it to the last chip would hide it.
    #[must_use]
    pub fn with_cursor(mut self, index: usize) -> Self {
        if index < self.chips.len() {
            self.cursor = Some(index);
        }
        self
    }

    /// Where the cursor rests — the seat a screen walked to, else the chip that
    /// is on, else the first chip. `None` only for an empty row.
    #[must_use]
    pub fn seat(&self) -> Option<usize> {
        if self.chips.is_empty() {
            return None;
        }
        Some(self.cursor.or_else(|| self.chosen()).unwrap_or(0))
    }

    /// The row's own tag — the group's node, and its Tab stop when the rule
    /// makes it a composite.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The row's accessible name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The rule this row declared.
    #[must_use]
    pub const fn choice(&self) -> Choice {
        self.choice
    }

    /// The chips, in the order they are painted and the order the cursor walks.
    #[must_use]
    pub fn chips(&self) -> &[Chip] {
        &self.chips
    }

    /// How many chips the row has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chips.len()
    }

    /// Whether the row has no chips.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chips.is_empty()
    }

    /// Which chips are on, member for member.
    #[must_use]
    pub fn on(&self) -> Vec<bool> {
        self.chips.iter().map(|chip| chip.on).collect()
    }

    /// The index of the chip that is on, when the rule allows at most one and
    /// one is.
    ///
    /// `None` for [`Choice::Any`], where "the chosen one" is not a question the
    /// row can answer — a caller that wants the set asks [`on`](Self::on).
    #[must_use]
    pub fn chosen(&self) -> Option<usize> {
        match self.choice {
            Choice::Any => None,
            Choice::AtMostOne | Choice::ExactlyOne => self.chips.iter().position(|chip| chip.on),
        }
    }

    /// The tags a screen's focus ring must contain for this row.
    ///
    /// One per chip when the members are independent, and the row's own tag
    /// when they are alternatives. This is the derivation a keyboard actually
    /// feels: the capture viewer's ring lost two stops to it, and gained a
    /// cursor and `Home` / `End` in exchange.
    #[must_use]
    pub fn stops(&self) -> Vec<&str> {
        if self.choice.is_composite() {
            vec![self.tag.as_str()]
        } else {
            self.chips.iter().map(|chip| chip.tag.as_str()).collect()
        }
    }

    /// Whether `tag` — the row's own or a chip's — is a keyboard stop.
    ///
    /// The read a painter wants, and the reason it is a question about a tag
    /// rather than two booleans: the row and its chips are answered by ONE
    /// derivation, so a screen cannot make the bar a stop and its chips stops as
    /// well. Measured before this existed: one screen had marked all three chips
    /// of an at-most-one row focusable by hand, and the bar was not a node at
    /// all.
    #[must_use]
    pub fn is_a_stop(&self, tag: &str) -> bool {
        self.stops().contains(&tag)
    }

    /// The cursor this row navigates by, seated where [`seat`](Self::seat) says.
    ///
    /// `None` in two cases, and they are different absences: the rule gives the
    /// row no cursor at all ([`Choice::Any`], whose members are reached by
    /// `Tab`), or the row is **empty** — a roster of nothing would advertise
    /// arrows that cannot move, which is the R1698 defect this vocabulary
    /// exists to prevent, and the group node still says the row is there.
    #[must_use]
    pub fn cursor(&self) -> Option<Roving> {
        if self.chips.is_empty() {
            return None;
        }
        let spec = self.choice.cursor()?;
        let mut roving = Roving::new(spec);
        roving.seat(
            self.chips
                .iter()
                .map(|chip| Member::maybe(chip.tag.clone(), chip.is_enabled()))
                .collect(),
        );
        if let Some(index) = self.seat() {
            let _ = roving.point_at(&self.chips[index].tag);
        }
        Some(roving)
    }

    /// Choose the chip at `index`: apply the rule, move the row, and say what
    /// happened.
    ///
    /// The three tones are the three things that can happen and they are not
    /// interchangeable — a chip that came on and a chip that went off are both
    /// [`Tone::Done`](crate::utterance::Tone::Done) and read differently, and a
    /// refusal is heard as one.
    pub fn choose(&mut self, index: usize) -> Utterance {
        if self.chips.get(index).is_some_and(|chip| !chip.is_enabled()) {
            return Utterance::refused(&RefusalClause {
                row: self,
                index,
                why: Refusal::Locked,
            });
        }
        match self.choice.apply(&self.on(), index) {
            Outcome::Set { on, now } => {
                for (chip, is_on) in self.chips.iter_mut().zip(on) {
                    chip.on = is_on;
                }
                let label = &self.chips[index].label;
                Utterance::done(if now {
                    format!("{label} on")
                } else {
                    format!("{label} off")
                })
            }
            Outcome::Refused(why) => Utterance::refused(&RefusalClause {
                row: self,
                index,
                why,
            }),
        }
    }
}

/// The sentence half of a [`Refusal`], which needs the row to write.
///
/// A separate type rather than a method on [`Refusal`] because the clause names
/// the chip and the row, and a reason that could be spelled without them would
/// be a reason a reader cannot act on — the class R1719 closed on the node lab
/// by naming a card the way the canvas does.
struct RefusalClause<'a> {
    row: &'a ChipGroup,
    index: usize,
    why: Refusal,
}

impl core::fmt::Display for RefusalClause<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.why {
            Refusal::NoSuchMember => write!(
                f,
                "{} has no chip {}, only {}",
                self.row.name(),
                self.index,
                self.row.len()
            ),
            Refusal::LastOneOn => write!(
                f,
                "{} is the only one on and {} keeps one on",
                self.row.chips[self.index].label,
                self.row.name()
            ),
            Refusal::Locked => write!(
                f,
                "{} cannot be changed here",
                self.row.chips[self.index].label
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utterance::Tone;

    fn row(choice: Choice, on: [bool; 3]) -> ChipGroup {
        ChipGroup::new(
            "row",
            "Saved filters",
            vec![
                Chip::new("row.0", "units only", on[0]),
                Chip::new("row.1", "shared memory", on[1]),
                Chip::new("row.2", "reassembly failed", on[2]),
            ],
            choice,
        )
    }

    // ── the derivations disagree, which is what makes them observable ──────

    /// ★★★★★ Every rule answers every derived question, and no two rules
    /// answer them all the same way. A derivation whose arms agreed would be
    /// a declaration a screen could ignore without anybody noticing.
    #[test]
    fn r1721_the_three_rules_are_distinguishable_by_what_they_derive() {
        let answers: Vec<_> = Choice::ALL
            .into_iter()
            .map(|choice| {
                let group = row(choice, [true, false, false]);
                (
                    choice.wire(),
                    choice.is_composite(),
                    choice.cursor().map(|spec| spec.activation),
                    group.stops().len(),
                    group.chosen(),
                )
            })
            .collect();
        for (i, left) in answers.iter().enumerate() {
            for right in &answers[i + 1..] {
                assert_ne!(left, right, "two rules derive the same answers");
            }
        }
        assert_eq!(answers.len(), Choice::ALL.len(), "every rule was walked");
    }

    /// Two rules that answered the same name would make a diagnostic unreadable.
    ///
    /// Distinctness rather than a round trip: [`Choice::wire`] is one-way by
    /// decision (see its doc), so a `from_wire` to round-trip against would be a
    /// parser for a message that cannot arrive — the R1719.1 shape.
    #[test]
    fn r1721_every_rule_has_its_own_name() {
        let names: Vec<_> = Choice::ALL.into_iter().map(Choice::wire).collect();
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "two rules share a name: {names:?}"
        );
        assert!(
            names.iter().all(|name| !name.is_empty()),
            "a rule with no name says nothing: {names:?}"
        );
    }

    // ── the stop count is the keyboard the rule buys ──────────────────────

    #[test]
    fn r1721_independent_chips_are_a_stop_each_and_alternatives_are_one() {
        assert_eq!(
            row(Choice::Any, [false; 3]).stops(),
            ["row.0", "row.1", "row.2"],
            "independent switches are reached by Tab"
        );
        for choice in [Choice::AtMostOne, Choice::ExactlyOne] {
            assert_eq!(
                row(choice, [true, false, false]).stops(),
                ["row"],
                "{}: a set of alternatives is one stop",
                choice.wire()
            );
        }
        // ★★★ One derivation answers for the row and for its chips, so a screen
        // cannot make both a stop — which is what "one Tab stop" means.
        for choice in Choice::ALL {
            let group = row(choice, [true, false, false]);
            assert_eq!(
                group.is_a_stop(group.tag()),
                choice.is_composite(),
                "{}",
                choice.wire()
            );
            assert_eq!(
                group.is_a_stop("row.1"),
                !choice.is_composite(),
                "{}",
                choice.wire()
            );
            assert!(
                !group.is_a_stop("somewhere.else"),
                "{}: a tag from another widget is not this row's stop",
                choice.wire()
            );
        }
    }

    /// The cursor rests where the row's answer is, so arriving at the row does
    /// not lose it — and it rests on the first chip when there is no answer yet.
    #[test]
    fn r1721_the_cursor_is_seated_on_the_chip_that_is_on() {
        let seated = row(Choice::AtMostOne, [false, true, false]);
        assert_eq!(seated.cursor().unwrap().cursor_tag(), Some("row.1"));
        let empty = row(Choice::AtMostOne, [false; 3]);
        assert_eq!(empty.cursor().unwrap().cursor_tag(), Some("row.0"));
        assert!(
            row(Choice::Any, [false; 3]).cursor().is_none(),
            "a row of independent switches has no cursor to lose"
        );
    }

    /// A chip nobody may choose is still walked to — the [`Member`] rule this
    /// row inherits, and the reason a locked seat is discoverable at all. What
    /// refuses is *choosing* it, and the refusal names the chip.
    #[test]
    fn r1721_a_locked_chip_takes_a_seat_and_refuses_the_choice() {
        let mut group = ChipGroup::new(
            "row",
            "Saved filters",
            vec![
                Chip::new("row.0", "units only", true),
                Chip::new("row.1", "shared memory", false).with_posture(ChipPosture::Locked),
            ],
            Choice::AtMostOne,
        );
        let cursor = group.cursor().unwrap();
        assert_eq!(cursor.members().len(), 2, "the locked chip has a seat");
        assert!(!cursor.members()[1].enabled, "and it is marked as locked");
        let said = group.choose(1);
        assert_eq!(said.tone(), Tone::Refused);
        assert!(
            said.clause().contains("shared memory"),
            "the refusal names the chip: {}",
            said.clause()
        );
        assert_eq!(group.on(), [true, false], "and nothing moved");
    }

    /// ★★★ A posture is one fact. The three booleans the paint and the
    /// accessibility tree read come from it, and `Locked` is the only one that
    /// is not enabled — so a chip cannot be locked and choosable at once.
    #[test]
    fn r1721_a_posture_answers_for_the_paint_and_the_tree_at_once() {
        let cases = [
            (ChipPosture::Idle, false, false, false, true),
            (ChipPosture::Hover, true, false, false, true),
            (ChipPosture::Pressed, false, true, false, true),
            (ChipPosture::Locked, false, false, true, false),
        ];
        for (posture, hovered, pressed, disabled, enabled) in cases {
            assert_eq!(posture.is_hovered(), hovered, "{posture:?}");
            assert_eq!(posture.is_pressed(), pressed, "{posture:?}");
            assert_eq!(posture.is_disabled(), disabled, "{posture:?}");
            assert_eq!(
                Chip::new("t", "l", false)
                    .with_posture(posture)
                    .is_enabled(),
                enabled,
                "{posture:?}"
            );
        }
    }

    /// The cursor a screen walked to survives being read back, and a seat the
    /// row does not have is not silently moved somewhere else.
    #[test]
    fn r1721_a_walked_cursor_is_kept_and_an_impossible_one_is_not_invented() {
        let walked = row(Choice::AtMostOne, [true, false, false]).with_cursor(2);
        assert_eq!(walked.seat(), Some(2));
        assert_eq!(walked.cursor().unwrap().cursor_tag(), Some("row.2"));
        assert_eq!(
            row(Choice::AtMostOne, [true, false, false])
                .with_cursor(9)
                .seat(),
            Some(0),
            "a seat off the end is ignored, and the chosen chip keeps the cursor"
        );
        let empty = ChipGroup::new("row", "Saved filters", Vec::new(), Choice::AtMostOne);
        assert_eq!(empty.seat(), None, "an empty row has no seat");
        assert!(
            empty.cursor().is_none(),
            "and no cursor, because arrows over nothing move nothing"
        );
        assert_eq!(
            empty.stops(),
            ["row"],
            "it is still a stop, so a reader can be told the row is empty"
        );
    }

    // ── what choosing does, per rule ───────────────────────────────────────

    #[test]
    fn r1721_any_toggles_the_one_chip_and_leaves_the_rest() {
        let mut group = row(Choice::Any, [true, false, false]);
        assert_eq!(group.choose(1).tone(), Tone::Done);
        assert_eq!(group.on(), [true, true, false], "both are on");
        assert_eq!(group.choose(0).tone(), Tone::Done);
        assert_eq!(group.on(), [false, true, false], "and one goes off alone");
    }

    #[test]
    fn r1721_at_most_one_replaces_and_can_be_emptied() {
        let mut group = row(Choice::AtMostOne, [true, false, false]);
        group.choose(2);
        assert_eq!(group.on(), [false, false, true], "choosing replaces");
        group.choose(2);
        assert_eq!(
            group.on(),
            [false; 3],
            "choosing the chosen one empties the row"
        );
        assert_eq!(group.chosen(), None);
    }

    /// ★★★★ The refusal the rule *is*. The floor cannot express this row at
    /// all: an exclusive set there keeps its member chosen and says nothing.
    #[test]
    fn r1721_exactly_one_refuses_to_be_emptied_and_says_why() {
        let mut group = row(Choice::ExactlyOne, [true, false, false]);
        group.choose(1);
        assert_eq!(group.on(), [false, true, false], "choosing replaces");
        let said = group.choose(1);
        assert_eq!(
            said.tone(),
            Tone::Refused,
            "clearing the last one is refused"
        );
        assert_eq!(group.on(), [false, true, false], "and nothing moved");
        assert!(
            said.clause().contains("shared memory") && said.clause().contains("Saved filters"),
            "the reason names the chip and the row, not an index: {}",
            said.clause()
        );
    }

    #[test]
    fn r1721_a_chip_that_is_not_there_is_refused_by_every_rule() {
        for choice in Choice::ALL {
            let mut group = row(choice, [true, false, false]);
            let said = group.choose(9);
            assert_eq!(said.tone(), Tone::Refused, "{}", choice.wire());
            assert_eq!(group.on(), [true, false, false], "{}", choice.wire());
            assert!(
                said.clause().contains("only 3"),
                "{}: the reason says how many there are: {}",
                choice.wire(),
                said.clause()
            );
        }
    }

    /// What a chip did is what is said — on and off are different sentences,
    /// because a reader who cannot see the pill has only the sentence.
    #[test]
    fn r1721_coming_on_and_going_off_are_different_sentences() {
        let mut group = row(Choice::Any, [false; 3]);
        let came_on = group.choose(0).into_clause();
        let went_off = group.choose(0).into_clause();
        assert_ne!(came_on, went_off);
        assert!(came_on.contains("units only") && went_off.contains("units only"));
    }

    /// `apply` is total: it answers for every rule at every index, including
    /// the ones off the end, and never panics.
    #[test]
    fn r1721_apply_answers_for_every_rule_at_every_index() {
        for choice in Choice::ALL {
            for index in 0..5 {
                let outcome = choice.apply(&[true, false, false], index);
                if index >= 3 {
                    assert_eq!(outcome, Outcome::Refused(Refusal::NoSuchMember));
                } else if let Outcome::Set { on, .. } = &outcome {
                    assert_eq!(on.len(), 3, "the set keeps its size");
                }
            }
        }
    }
}
