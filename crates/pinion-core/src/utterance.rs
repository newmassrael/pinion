//! R1719 §5.12 §5.15 §2 #7 — **what a screen just said to a person**, as a
//! value that knows what kind of thing it is.
//!
//! # The defect this exists for, measured by driving the screens
//!
//! Three screens of the analysis tool each keep "the last thing I said" and
//! each keeps it as a `String`. Measured 2026-08-18 by running them: **117 call
//! sites** hand a string to a one-line setter (58 · 11 · 48), and the wire
//! types all three as `string`. Nothing downstream can ask what KIND of thing
//! was said, so nothing downstream does — with two consequences a reader
//! actually meets:
//!
//! * **A refusal is announced with the same urgency as a confirmation**,
//!   because the urgency is a per-screen constant. Driven: selecting a card on
//!   the node lab says `selected R-01` and the live region reads `assertive` —
//!   a screen reader is *interrupted* to be told something the person just did
//!   on purpose. The other two screens are `polite`, so on those a refusal
//!   waits for a pause that a person working the tool does not leave. The same
//!   pair of facts, filed under the wrong halves.
//! * **An act that changed nothing says nothing at all**, and the previous
//!   message stands. Driven: renaming a card to the name it already has leaves
//!   the *earlier refusal* on screen, so a person reads a sentence about a
//!   different act and cannot tell it is stale.
//!
//! And the fact that a refusal IS a refusal was being carried in a **string
//! prefix**: `format!("refused: {sentence}")` at five sites on one screen, a
//! `refusal_sentence` helper on another, `format!("query refused: {why}")` on
//! the third. That is R1717's defect — a fact smuggled into wording, recovered
//! by whoever remembers to sniff for it — one level up from where R1718 closed
//! it.
//!
//! # What the floor does, measured rather than read
//!
//! A probe was built against the mature toolkit at 6.11.1 and **run**
//! offscreen:
//!
//! | question | answer |
//! |---|---|
//! | does its status channel carry a kind? | **no** — 62 properties + 37 methods, **0** name one |
//! | can a caller ask what kind the last message was? | **no** — the accessor answers the text |
//! | its dialogs DO have a kind (5 arms) — does the kind change what a reader hears? | **no** — the accessible role is the same number for the critical and the informational one |
//! | does its announcement event carry urgency? | **yes**, and the caller passes it, derived from nothing; the default is the polite one |
//! | does anything refuse a doubly-framed or empty message? | **no** — `refused: refused: refused: ` and `""` are both accepted verbatim |
//!
//! So an urgency *exists* there and a kind *exists* there, and **nothing joins
//! them**: the one place the toolkit keeps a frame and a clause apart is a
//! modal dialog's primary and secondary text, which says nothing about why they
//! are two. Six capabilities this module ships are compile errors there: asking
//! the channel what kind the last message was, saying something *as* a refusal,
//! deriving the urgency from the kind, refusing a doubly-framed message,
//! reading the producer's own clause without the frame, and saying "it was
//! already so".
//!
//! # The shape
//!
//! [`Utterance`] is a [`Tone`] and a **clause** — the producer's own sentence,
//! stored without any frame. [`Utterance::sentence`] composes the two for a
//! reader; [`Utterance::clause`] gives the producer's half back untouched. So
//! the frame is never in the stored text and no later reader has to take it
//! off.
//!
//! The rule about what may be said lives in exactly one place
//! ([`Utterance::fault`]) behind two doors, the pattern
//! [`shrink`](crate::shrink) established: [`Utterance::checked`] returns the
//! fault and is what tests drive, and the ordinary constructors panic with the
//! fault's own sentence, because a screen composing a malformed announcement is
//! a programming error and not a thing to render.

use serde::Serialize;
use serde::ser::SerializeStruct;
use std::borrow::Cow;
use std::fmt;

/// What kind of thing a screen said.
///
/// Three arms, and each is a situation the analysis tool's screens were
/// measured saying. They are not degrees of severity: [`Refused`](Self::Refused)
/// is the only one where the person's act did not happen, and
/// [`Unchanged`](Self::Unchanged) is the only one where it did not need to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Tone {
    /// It happened. The clause says what.
    Done,
    /// It did not happen, and the clause says why.
    ///
    /// The only arm that frames its clause — see [`Tone::frame`].
    Refused,
    /// It was already so, so nothing changed.
    ///
    /// Distinct from [`Done`](Self::Done) because a person who cannot tell them
    /// apart cannot tell whether the tool heard them: measured on the node lab,
    /// renaming a card to its own name left the previous message standing.
    Unchanged,
}

impl Tone {
    /// Every kind of thing a screen can say, in declaration order.
    pub const ALL: [Self; 3] = [Self::Done, Self::Refused, Self::Unchanged];

    /// The name this tone carries on the wire.
    ///
    /// Chosen once rather than derived from the Rust identifier, because an
    /// agent matches on it.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Refused => "refused",
            Self::Unchanged => "unchanged",
        }
    }

    /// The tone [`wire`](Self::wire) spells this way, or `None`.
    ///
    /// The other half of the wire form. A tone that can be written and not read
    /// is a tone whose round trip nothing can check, and this type goes through
    /// a reactive cell that serializes.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tone| tone.wire() == name)
    }

    /// The word this tone puts in front of a clause, or `None` when it puts
    /// nothing there.
    ///
    /// ★★★★★ **Exactly one tone may frame nothing**, and the round that built
    /// this type got it wrong: the first draft framed only
    /// [`Refused`](Self::Refused), on the grounds that `refused: ` was the only
    /// word the three screens had written by hand. R1718's speech gate refused
    /// that draft, and it was right — with no frame of its own,
    /// [`Unchanged`](Self::Unchanged) is **inaudible**. It shares its urgency
    /// with [`Done`](Self::Done), so a reader who cannot see the bullet's
    /// colour has nothing at all to tell the two apart, and the whole reason
    /// the arm exists is that a person could not tell whether their act had
    /// done anything.
    ///
    /// `no change` rather than `already`, measured against the five situations
    /// that reach it: two of them are not about a thing that was already so but
    /// about there being nothing to do (`nothing to go to — the gate is clear`),
    /// and `already: nothing to go to` is not a sentence.
    #[must_use]
    pub const fn frame(self) -> Option<&'static str> {
        match self {
            Self::Refused => Some("refused"),
            Self::Unchanged => Some("no change"),
            Self::Done => None,
        }
    }

    /// How urgently a reader should hear this.
    ///
    /// **Derived, so a screen cannot choose it.** That is the whole point: the
    /// three screens were measured choosing a constant, and each got it wrong
    /// for half of what it said.
    #[must_use]
    pub const fn urgency(self) -> Urgency {
        match self {
            // The person's act did not happen. Waiting for a pause means they
            // carry on believing it did.
            Self::Refused => Urgency::Interrupting,
            // The answer to something they just did on purpose. Worth saying,
            // not worth cutting anyone off for.
            Self::Done | Self::Unchanged => Urgency::WhenIdle,
        }
    }

    /// The colour role a reader who can SEE this learns the tone from.
    ///
    /// ★★★★★ R2012 — the seeing half of [`urgency`](Self::urgency), derived for
    /// the same reason and by the same argument. A tone is heard through its
    /// urgency, which a screen cannot choose; it is seen through a mark whose
    /// colour, until now, every screen picked for itself.
    ///
    /// A screen was measured picking wrong. The analysis shell's status bullet
    /// drew [`Done`](Self::Done) in [`InversePrimary`](crate::theme::ColorRole::InversePrimary)
    /// — an accent tone whose declared ground is
    /// [`InverseSurface`](crate::theme::ColorRole::InverseSurface), not the
    /// plain surface it sits on. It was not a careless choice: there was no
    /// role for *it happened* to reach for, so the nearest tone that was not
    /// the error red got used.
    ///
    /// ⚠ AND THE POPULATION IS THE DEFAULT PALETTES, NOT THAT SCREEN — a
    /// counterfactual is what said so. Against [`crate::theme::Theme::light`]
    /// and [`crate::theme::Theme::dark`] that pairing reads **1.70** and
    /// **2.17**, under even the 3.0 a non-text mark is held to; the shell binds
    /// a magenta of its own for the role and read 7.88 and 5.97, so the screen
    /// that HAD the wrong mapping was not the screen the wrong mapping showed
    /// on. Putting the mapping here is what makes that distinction stop
    /// mattering: the next consumer inherits the answer instead of the search.
    ///
    /// ⚠ [`Unchanged`](Self::Unchanged) deliberately answers
    /// [`OnSurfaceMuted`](crate::theme::ColorRole::OnSurfaceMuted) rather than
    /// [`Info`](crate::theme::ColorRole::Info), and the
    /// distinction is the arm's own reason for existing: *nothing changed* is
    /// about the person's act having had nothing to do, not about a fact the
    /// screen is volunteering. Painting it in the informational tone would give
    /// a non-event the same weight as a notice.
    #[must_use]
    pub const fn role(self) -> crate::theme::ColorRole {
        use crate::theme::ColorRole;
        match self {
            Self::Done => ColorRole::Success,
            Self::Refused => ColorRole::Error,
            Self::Unchanged => ColorRole::OnSurfaceMuted,
        }
    }
}

/// How urgently an utterance should reach a reader who is not looking at it.
///
/// Two arms where the accessibility vocabulary has three: there is no "off",
/// because an utterance is a thing a screen chose to say and a screen that
/// wants silence does not say it. The third arm belongs to a *declared region*,
/// which can be nested inside a live ancestor and opt out;
/// `pinion_a11y::AccessLive` is where that lives and where this lowers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Urgency {
    /// Announced when the reader is idle, without interrupting.
    WhenIdle,
    /// Announced immediately, interrupting whatever is being read.
    Interrupting,
}

impl Urgency {
    /// Every urgency, in declaration order.
    pub const ALL: [Self; 2] = [Self::WhenIdle, Self::Interrupting];

    /// The name this urgency carries on the wire.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::WhenIdle => "when-idle",
            Self::Interrupting => "interrupting",
        }
    }
}

/// Why a clause cannot be said.
///
/// Each arm is a defect this tree shipped, not a category invented for
/// symmetry — see the per-arm notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Fault {
    /// Nothing was said.
    ///
    /// A screen that hands an empty clause to its channel has decided to
    /// announce and then announced nothing, which paints an empty toast and
    /// tells a reader an empty live region changed.
    Empty,
    /// The clause already begins with this tone's own frame, so the composed
    /// sentence would say it twice.
    ///
    /// The producers here return sentences, and a caller that frames a sentence
    /// which framed itself gets `refused: refused: …`. The floor accepts that
    /// verbatim, measured.
    AlreadyFramed,
    /// The clause is a Rust value's `Debug` spelling rather than a sentence.
    ///
    /// R1699 measured this reaching a person eight times —
    /// `Rejected(RefusalReason("\"topology\" is reserved …"))` on screen, Rust
    /// syntax and escaped quotes and all — and fixed it with a helper each
    /// screen has to remember to call. Its own note says a screen that has to
    /// remember not to use `Debug` is a screen that will use `Debug`, so the
    /// remembering is moved here.
    DebugSpelling,
}

impl Fault {
    /// Every reason a clause cannot be said, in declaration order.
    pub const ALL: [Self; 3] = [Self::Empty, Self::AlreadyFramed, Self::DebugSpelling];

    // ★★ R1719.1 (closing audit) — a `wire()` used to sit here, beside
    // `Tone::wire` and `Urgency::wire`, and it had **no consumer**: a fault
    // never crosses the wire. It is what `checked` hands a test and what the
    // production door panics with, and `Utterance`'s serializer does not carry
    // it, so the name was symmetry with two neighbours that really are
    // published. Deleted on R1706.1's rule — parity is about a capability, not
    // about a method having a peer.

    /// What a developer reads when this fault stops an utterance.
    ///
    /// A clause, in the sense this workspace uses the word: the caller supplies
    /// the subject (the clause itself), so no sentence here names it.
    #[must_use]
    pub fn sentence(self) -> String {
        match self {
            Self::Empty => "says nothing".to_owned(),
            Self::AlreadyFramed => "already carries the frame this tone adds".to_owned(),
            Self::DebugSpelling => "is a Rust value's Debug spelling, not a sentence".to_owned(),
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence())
    }
}

/// The last thing a screen said to a person.
///
/// Constructed through [`done`](Self::done), [`refused`](Self::refused) and
/// [`unchanged`](Self::unchanged), which is why there is no way to hold a tone
/// and a clause that disagree about framing: the frame is never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    /// What kind of thing this is.
    tone: Tone,
    /// The producer's own sentence, with no frame on it.
    clause: String,
}

impl Utterance {
    /// Why `clause` cannot be said in `tone`, or `None` when it can.
    ///
    /// **The rule, in one place.** Both doors below go through here, so a
    /// screen and a test cannot disagree about what is sayable.
    #[must_use]
    pub fn fault(tone: Tone, clause: &str) -> Option<Fault> {
        let trimmed = clause.trim();
        if trimmed.is_empty() {
            return Some(Fault::Empty);
        }
        if let Some(frame) = tone.frame() {
            let head = trimmed.to_lowercase();
            if head
                .strip_prefix(frame)
                .is_some_and(|rest| rest.starts_with(':') || rest.starts_with(char::is_whitespace))
            {
                return Some(Fault::AlreadyFramed);
            }
        }
        if is_debug_spelling(trimmed) {
            return Some(Fault::DebugSpelling);
        }
        None
    }

    /// `clause`, said in `tone`, or the reason it cannot be.
    ///
    /// The testable door. Production code takes one of the three named
    /// constructors, which panic instead — see [`new`](Self::new).
    ///
    /// # Errors
    ///
    /// Returns whatever [`fault`](Self::fault) answers.
    pub fn checked(tone: Tone, clause: impl Into<String>) -> Result<Self, Fault> {
        let clause = clause.into();
        match Self::fault(tone, &clause) {
            Some(fault) => Err(fault),
            None => Ok(Self {
                tone,
                clause: clause.trim().to_owned(),
            }),
        }
    }

    /// `clause`, said in `tone`.
    ///
    /// # Panics
    ///
    /// When [`fault`](Self::fault) answers, with that fault's own sentence. A
    /// screen composing a malformed announcement is a programming error: there
    /// is no rendering of it that helps the person in front of it, and swapping
    /// in a placeholder would put a lie on the screen where the defect was.
    #[must_use]
    pub fn new(tone: Tone, clause: impl Into<String>) -> Self {
        let clause = clause.into();
        match Self::checked(tone, clause.clone()) {
            Ok(said) => said,
            Err(fault) => panic!("{tone:?} clause {clause:?} {}", fault.sentence()),
        }
    }

    /// It happened, and this is what.
    #[must_use]
    pub fn done(clause: impl Into<String>) -> Self {
        Self::new(Tone::Done, clause)
    }

    /// It did not happen, and this is why.
    ///
    /// Takes anything that can say itself, so the producer's `Display` reaches
    /// the person rather than its `Debug` — and a `Debug` spelling handed in
    /// anyway is [`Fault::DebugSpelling`] rather than something a reader has to
    /// decipher.
    #[must_use]
    pub fn refused(why: &impl fmt::Display) -> Self {
        Self::new(Tone::Refused, why.to_string())
    }

    /// It was already so, and this is what was already so.
    #[must_use]
    pub fn unchanged(clause: impl Into<String>) -> Self {
        Self::new(Tone::Unchanged, clause)
    }

    /// What kind of thing this is.
    #[must_use]
    pub const fn tone(&self) -> Tone {
        self.tone
    }

    /// The producer's own sentence, without this tone's frame.
    ///
    /// The half a test asserts on when it cares what happened rather than how
    /// it was framed — which is what the framing being a value buys.
    #[must_use]
    pub fn clause(&self) -> &str {
        &self.clause
    }

    /// The producer's own sentence, taken out of this utterance.
    ///
    /// The shape a refusal reaches an agent by: the person is told
    /// `refused: …` and the agent's channel is *already* a refusal, so it gets
    /// the clause alone. One value answers both, which is the point — the two
    /// used to be two `format!`s that could drift.
    #[must_use]
    pub fn into_clause(self) -> String {
        self.clause
    }

    /// What a person reads: the tone's frame, then the clause.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self.tone.frame() {
            Some(frame) => format!("{frame}: {}", self.clause),
            None => self.clause.clone(),
        }
    }

    /// How urgently a reader should hear this — [`Tone::urgency`], derived.
    #[must_use]
    pub const fn urgency(&self) -> Urgency {
        self.tone.urgency()
    }
}

impl fmt::Display for Utterance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence())
    }
}

/// The wire form: the value, plus the two things a reader would otherwise have
/// to re-derive.
///
/// Hand-written rather than derived so there is **one** spelling of each name.
/// A `#[serde(rename_all)]` beside [`Tone::wire`] would be a second record of
/// the same decision, and this workspace has spent several rounds on pairs like
/// that disagreeing.
///
/// `sentence` is what a person reads and `clause` is what the producer said, so
/// a test that cares about the fact can assert on the clause and one that cares
/// about the wording can assert on the sentence, without either taking a prefix
/// off a string.
impl Serialize for Utterance {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut row = s.serialize_struct("Utterance", 4)?;
        // ★ R1719.1 (closing audit) — through the accessors, not the fields.
        // `clause()` had no reader outside this module's tests while the wire
        // published the same fact off the field beside it, which is two readings
        // of one record with nothing holding them together.
        row.serialize_field("tone", self.tone().wire())?;
        row.serialize_field("clause", self.clause())?;
        row.serialize_field("sentence", &self.sentence())?;
        row.serialize_field("urgency", self.urgency().wire())?;
        row.end()
    }
}

/// Reading one back keeps only what is not derived, and **re-applies the
/// rule**: a stored utterance that violates it is refused rather than revived.
///
/// The two derived fields are ignored on the way in on purpose. Accepting a
/// `sentence` that disagrees with `tone` + `clause` would be the two-records
/// defect this type exists to remove, arriving through the back door.
impl<'de> serde::Deserialize<'de> for Utterance {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Halves {
            tone: String,
            clause: String,
        }
        let halves = Halves::deserialize(d)?;
        let tone = Tone::from_wire(&halves.tone)
            .ok_or_else(|| serde::de::Error::custom(format!("{:?} is not a tone", halves.tone)))?;
        Self::checked(tone, halves.clause)
            .map_err(|fault| serde::de::Error::custom(fault.sentence()))
    }
}

/// Whether `clause` reads as a Rust value's `Debug` spelling.
///
/// The shape R1699 measured on screen: an upper-case initial identifier
/// followed **immediately** by `(` or `{`. The "immediately" is what keeps an
/// ordinary sentence out — `Router (R-01) is already running` has a space
/// there, and `Rejected(RefusalReason(…))` does not.
///
/// Deliberately not a general Rust-syntax test. This looks for the one
/// construction that reached a person, so an odd sentence is passed rather than
/// argued with: a gate that argues gets switched off.
fn is_debug_spelling(clause: &str) -> bool {
    let head: String = clause
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if head.len() < 2 || !head.starts_with(|c: char| c.is_ascii_uppercase()) {
        return false;
    }
    matches!(clause[head.len()..].chars().next(), Some('(' | '{'))
}

/// ★★★★★ R1720 — **what a surface did about telling the person that a call was
/// refused**, answered to the framework rather than decided by each screen.
///
/// # The defect this exists for, measured by driving the screens
///
/// Measured 2026-08-18 by driving every action slot the three analysis screens
/// publish, with an argument each verb must refuse: **55 verbs refuse and 2
/// reach the person** (node lab 2 of 26, capture viewer 0 of 9, dashboard 0 of
/// 20). The two that arrive do so because that screen hand-wrote the coupling
/// at those two sites — `let said = …; state.say(said.clone()); Err(…)` — so
/// the property held exactly where somebody remembered it, which is the same
/// shape as every fact this workspace has since moved into a type.
///
/// §2 #2 makes RPC the AI's primary path, so "an agent drives and a person
/// watches" is the *normal* state of these screens, not an exception. In that
/// state a refused call left the screen still holding a sentence about some
/// earlier act, and the person had no way to know why nothing moved — the same
/// stale-message defect [`Tone::Unchanged`] was built for, arriving through the
/// other channel.
///
/// # Why three arms
///
/// [`Nowhere`](Self::Nowhere) and [`Undeclared`](Self::Undeclared) are not one
/// arm, for R1718's reason: if "nothing here speaks to a person" were also the
/// answer of a surface that never considered the question, then a census could
/// be satisfied by never considering it, and the useful half — *which* surfaces
/// decided they have no live region, and why — would be unreadable. So the
/// framework's default is its own arm and no surface writes it.
///
/// # Why [`At`](Self::At) names a place
///
/// So the claim can be checked rather than believed. The tag it carries is a
/// live region in the surface's own access tree, and the harness gate goes and
/// reads the sentence there — a surface cannot satisfy the gate by answering
/// `At` and doing nothing.
///
/// # The floor, measured rather than read
///
/// A probe was built against the mature toolkit at 6.11.1 and **run**
/// offscreen. Refusing a programmatic call emits **0** accessibility events —
/// both for a verb nobody declared and for a declared verb that answers "no" —
/// and the caller's whole answer is a boolean; the *reason* for the first goes
/// to the process's diagnostic stream, a global sink no caller and no person
/// reads. Its status channel emits **0** accessibility events of its own, takes
/// no kind, and nothing routes a refusal into it: a caller that wants the
/// person told has to call it, with words it composes itself. And nothing
/// anywhere reports whether the person was told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Announced {
    /// The person was told, in the live region this names.
    At(Cow<'static, str>),
    /// This surface has nowhere to put speech, and this says why.
    ///
    /// A reason and not a bare marker, for the reason every exemption in this
    /// workspace carries one: a silent list can be grown by adding a name, and
    /// a reason cannot be written without deciding something.
    Nowhere(Cow<'static, str>),
    /// Nobody has decided. The framework's own default, and the only answer no
    /// surface writes for itself.
    Undeclared,
}

impl Announced {
    /// The person was told, in the live region tagged `tag`.
    #[must_use]
    pub fn at(tag: impl Into<Cow<'static, str>>) -> Self {
        Self::At(tag.into())
    }

    /// Nothing on this surface speaks to a person, `because` this.
    #[must_use]
    pub fn nowhere(because: impl Into<Cow<'static, str>>) -> Self {
        Self::Nowhere(because.into())
    }

    /// The live region carrying the sentence, when there is one.
    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        match self {
            Self::At(tag) => Some(tag),
            Self::Nowhere(_) | Self::Undeclared => None,
        }
    }

    /// The word this answer carries on the wire.
    ///
    /// Chosen rather than derived from the Rust identifier, for
    /// [`Tone::wire`]'s reason: an agent matches on it.
    #[must_use]
    pub const fn wire(&self) -> &'static str {
        match self {
            Self::At(_) => "at",
            Self::Nowhere(_) => "nowhere",
            Self::Undeclared => "undeclared",
        }
    }
}

/// The wire form: the answer's own word, and whatever second fact that word has.
///
/// Hand-written for [`Utterance`]'s reason — one spelling of each name, beside
/// [`Announced::wire`] rather than in a `serde` attribute that would be a
/// second record of the same decision.
impl Serialize for Announced {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut row = s.serialize_struct("Announced", 2)?;
        row.serialize_field("reach", self.wire())?;
        match self {
            Self::At(tag) => row.serialize_field("at", tag.as_ref())?,
            Self::Nowhere(why) => row.serialize_field("because", why.as_ref())?,
            Self::Undeclared => row.skip_field("at")?,
        }
        row.end()
    }
}

/// ★★★★★ R1778 §5.12 §5.28 — **what a screen is saying, and for how much
/// longer.**
///
/// # The defect this exists for, reported by a person
///
/// A reader ran this tree's assembled analysis tool and found two status boxes
/// stacked at the foot of a mounted screen, over its palette and over its own
/// hint strip — the host's and the guest's, both permanent. The overlap was the
/// symptom. The defect was that NEITHER ONE EVER LEFT, and the reference this
/// tool reproduces clears its own after 2600ms.
///
/// # Why the LIFETIME is the thing that lifted, and not the box
///
/// Three screens paint a status message and all three paint it differently — a
/// floating box on a canvas, a label in an application bar, a box at a page's
/// foot. That difference is real and a single widget over it would fight the
/// reference, which is why the debt recording it deferred a lift twice.
///
/// The lifetime is not like that. It is not three different lifetimes; it is
/// the same one absent three times, with the same number behind it. So this
/// holds WHEN a sentence stops being current and WHERE it is kept, and every
/// screen keeps its own painter.
///
/// # It also carries the holder, which is the half that was silently divergent
///
/// Measured before the lift: one screen held `Signal<Utterance>`, one
/// `Signal<Option<Utterance>>`, and one `RefCell<Option<Utterance>>`. The third
/// is the one that matters — **a `RefCell` is not reactive**, so a lifetime
/// added to that screen alone would expire without anything repainting, and the
/// screen would look unfixed while its code said otherwise.
///
/// # The clock is the framework's, and it is not `view`
///
/// [`Tickable`](crate::animation::Tickable), advanced by
/// [`Owner::tick_animations`](crate::reactive::Owner::tick_animations) once per
/// paint with the frame's `dt`. It cannot be `view`: §6.3 makes the view
/// function sync and pure so `dry_run` holds, and a message that aged itself
/// while being drawn would trade a binding invariant for a repair.
///
/// # One reactive slot, deliberately
///
/// Expiry writes `None` into the sentence, and that write IS the repaint
/// signal, so the remaining time can be a plain [`Cell`](std::cell::Cell). The
/// first screen to gain a lifetime carried two signals and a screen-local
/// ticker; adopting this removes both. A lift that only ever adds is usually
/// cutting along the wrong axis.
pub struct Saying {
    said: crate::reactive::Signal<Option<Utterance>>,
    left: std::cell::Cell<f32>,
    life: f32,
}

impl Saying {
    /// A holder that keeps what it is told for `life` seconds, saying nothing
    /// yet.
    #[must_use]
    pub fn new(life: f32) -> Self {
        Self {
            said: crate::reactive::Signal::new(None),
            left: std::cell::Cell::new(0.0),
            life,
        }
    }

    /// Say something, and start its life over.
    ///
    /// Restarting rather than queueing is what makes a second message REPLACE a
    /// first: a person watching a screen reads the latest thing that happened,
    /// and a queue would show them a stale sentence while a fresh one waited.
    pub fn say(&self, what: Utterance) {
        self.said.set(Some(what));
        self.left.set(self.life);
    }

    /// What is being said right now, or `None` once its time is up.
    #[must_use]
    pub fn showing(&self) -> Option<Utterance> {
        self.said.get()
    }

    /// What a person reads, or an empty string when nothing is current.
    ///
    /// The spelling every screen's accessibility value and wire slot wants, so
    /// three of them do not write three `unwrap_or_default` chains.
    #[must_use]
    pub fn sentence(&self) -> String {
        self.said
            .get()
            .map(|said| said.sentence())
            .unwrap_or_default()
    }

    /// How long the current sentence has left, in seconds.
    ///
    /// Zero when nothing is being said. Published because a screen's own gate
    /// may want to drive time forward deliberately, and a test that guesses the
    /// duration is a test that pins a number this type owns.
    #[must_use]
    pub fn left(&self) -> f32 {
        self.left.get()
    }

    /// The whole life a sentence is given, in seconds.
    #[must_use]
    pub const fn life(&self) -> f32 {
        self.life
    }

    /// ★★★★★ R1790 — **the sentence AND how long it has**, as one wire value.
    ///
    /// # What this is the repair of, measured
    ///
    /// [`left`](Self::left)'s own doc already argued for this — *"published
    /// because a screen's own gate may want to drive time forward
    /// deliberately, and a test that guesses the duration is a test that pins a
    /// number this type owns"* — and the fact was reachable only from Rust. A
    /// harness that talks over the wire could not ask, so it guessed, and a
    /// guess about time is a check whose verdict depends on machine speed.
    ///
    /// R1787's CI run failed exactly that way. Three demos take a baseline of
    /// what a screen paints and compare it against a later read; the analysis
    /// shell says `Overview loaded` at boot, so its toast was IN the baseline
    /// and gone by the comparison on a slow runner — reported as a region the
    /// reader cannot bring into view. Measured here: 214 regions with the toast,
    /// 213 without. The demo passes locally in 8 seconds and failed in CI, which
    /// is the signature of a check that reads a clock it cannot see.
    ///
    /// With this on the wire the harness settles by **asking**: advance until
    /// `left` is zero, then take the baseline. No lifetime constant in the
    /// harness, no per-screen exemption table, and no number that goes stale
    /// when a screen changes its mind about how long it speaks for.
    ///
    /// # Against the floor
    ///
    /// The reference's status bar takes a timeout when it is given a message
    /// and offers no way to ask how much of it is left — a caller can read the
    /// current text and cannot tell a sentence about to vanish from one that
    /// just arrived. This publishes both, and `showing` stays the answer to
    /// *is anything being said at all*.
    #[must_use]
    pub fn to_wire(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "said".to_owned(),
            serde_json::to_value(self.showing()).unwrap_or(serde_json::Value::Null),
        );
        obj.insert("left".to_owned(), serde_json::json!(self.left()));
        obj.insert("life".to_owned(), serde_json::json!(self.life()));
        serde_json::Value::Object(obj)
    }
}

impl crate::animation::Tickable for Saying {
    fn tick(&self, dt: f32) {
        if self.said.get().is_none() {
            return;
        }
        let left = self.left.get() - dt;
        if left <= 0.0 {
            self.left.set(0.0);
            // The write that is also the repaint.
            self.said.set(None);
        } else {
            self.left.set(left);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // A sentence with time left is NOT at rest even while nothing on screen
        // is moving: what it is waiting for is its own expiry, and a driver
        // that skipped it would leave the message up forever — which is the
        // defect this type exists for.
        self.said.get().is_none()
    }
}

impl fmt::Debug for Saying {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Saying")
            .field("said", &self.said.get())
            .field("left", &self.left.get())
            .field("life", &self.life)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_stores_the_clause_without_the_frame_it_shows() {
        let said = Utterance::refused(&"\"P-01\" is already taken");
        assert_eq!(said.clause(), "\"P-01\" is already taken");
        assert_eq!(said.sentence(), "refused: \"P-01\" is already taken");
    }

    #[test]
    fn exactly_one_tone_frames_nothing() {
        let bare: Vec<Tone> = Tone::ALL
            .into_iter()
            .filter(|tone| tone.frame().is_none())
            .collect();
        assert_eq!(
            bare,
            vec![Tone::Done],
            "two tones that frame nothing are two tones a reader cannot tell \
             apart, which is what the speech gate refused this module's first \
             draft for"
        );
        let said = Utterance::done("R-01 moved");
        assert_eq!(said.sentence(), said.clause(), "and it shows its clause");
    }

    #[test]
    fn a_clause_that_already_framed_itself_is_refused() {
        for spelling in ["refused: taken", "Refused: taken", "refused taken"] {
            assert_eq!(
                Utterance::checked(Tone::Refused, spelling),
                Err(Fault::AlreadyFramed),
                "{spelling:?}"
            );
        }
    }

    #[test]
    fn the_frame_is_only_a_fault_for_the_tone_that_adds_it() {
        // The same words are sayable when nothing is going to prepend them.
        assert!(Utterance::checked(Tone::Done, "refused: taken").is_ok());
    }

    #[test]
    fn a_word_that_merely_starts_with_the_frame_is_not_framed() {
        let said = Utterance::checked(Tone::Refused, "refusedness is not a word")
            .expect("only the whole frame word counts");
        assert_eq!(said.sentence(), "refused: refusedness is not a word");
    }

    #[test]
    fn nothing_said_is_a_fault_in_every_tone() {
        for tone in Tone::ALL {
            assert_eq!(
                Utterance::checked(tone, "   "),
                Err(Fault::Empty),
                "{tone:?}"
            );
        }
    }

    #[test]
    fn a_debug_spelling_is_refused_and_the_sentence_it_wraps_is_not() {
        assert_eq!(
            Utterance::checked(
                Tone::Refused,
                "Rejected(RefusalReason(\"\\\"topology\\\" is reserved\"))"
            ),
            Err(Fault::DebugSpelling),
        );
        assert!(Utterance::checked(Tone::Refused, "\"topology\" is reserved").is_ok());
    }

    #[test]
    fn a_sentence_with_a_bracket_after_a_space_is_not_a_debug_spelling() {
        assert!(Utterance::checked(Tone::Done, "Router (R-01) is running").is_ok());
        assert!(Utterance::checked(Tone::Done, "R-01 moved (24, 8)").is_ok());
    }

    #[test]
    fn a_refusal_interrupts_and_the_other_two_wait() {
        assert_eq!(Tone::Refused.urgency(), Urgency::Interrupting);
        assert_eq!(Tone::Done.urgency(), Urgency::WhenIdle);
        assert_eq!(Tone::Unchanged.urgency(), Urgency::WhenIdle);
    }

    #[test]
    fn every_tone_and_urgency_has_its_own_wire_name() {
        let tones: std::collections::BTreeSet<&str> = Tone::ALL.iter().map(|t| t.wire()).collect();
        assert_eq!(tones.len(), Tone::ARMS, "one wire name per tone");
        let urgencies: std::collections::BTreeSet<&str> =
            Urgency::ALL.iter().map(|u| u.wire()).collect();
        assert_eq!(urgencies.len(), Urgency::ARMS, "one wire name per urgency");
    }

    #[test]
    #[should_panic(expected = "says nothing")]
    fn the_production_door_panics_with_the_faults_own_sentence() {
        let _ = Utterance::done("");
    }

    /// ★★★★ R1719 — found by a counterfactual that PASSED: the wire form had
    /// no test in this crate at all, so a `serialize_field("tone", …)` that
    /// always wrote `done` was invisible here. A wire that answers one tone is
    /// the string channel again with more fields on it.
    #[test]
    fn the_wire_form_carries_the_tone_that_was_said() {
        for tone in Tone::ALL {
            let said = Utterance::new(tone, "R-01 is where it was");
            let json = serde_json::to_value(&said).expect("an utterance serializes");
            assert_eq!(json["tone"], tone.wire(), "{tone:?} names itself");
            assert_eq!(json["clause"], said.clause(), "{tone:?} carries the clause");
            assert_eq!(json["sentence"], said.sentence(), "{tone:?} composes");
            assert_eq!(
                json["urgency"],
                tone.urgency().wire(),
                "{tone:?} carries the urgency it derives"
            );
        }
    }

    /// The round trip keeps the two halves that are not derived, and nothing
    /// else — a `sentence` on the way in would be a second record of a fact
    /// the other two already decide.
    #[test]
    fn a_value_read_back_is_the_value_that_was_written() {
        for tone in Tone::ALL {
            let said = Utterance::new(tone, "\"P-01\" is already taken");
            let text = serde_json::to_string(&said).expect("serializes");
            let back: Utterance = serde_json::from_str(&text).expect("deserializes");
            assert_eq!(back, said, "{tone:?} survives the wire");
        }
    }

    /// ★★★★ R1719 — also found by a passing counterfactual. A stored utterance
    /// that breaks the rule is **refused**, not revived: this type goes through
    /// a reactive cell that serializes, so the back door is a real one.
    #[test]
    fn a_stored_value_that_breaks_the_rule_is_refused_rather_than_revived() {
        for (stored, why) in [
            (
                r#"{"tone":"refused","clause":"refused: taken"}"#,
                "already framed",
            ),
            (r#"{"tone":"done","clause":"   "}"#, "empty"),
            (r#"{"tone":"nonsense","clause":"x"}"#, "not a tone"),
        ] {
            assert!(
                serde_json::from_str::<Utterance>(stored).is_err(),
                "{stored} is {why} and must not come back"
            );
        }
    }

    /// ★★★★ R1719 — the third passing counterfactual. `into_clause` is what an
    /// agent's channel receives, and that channel is already a refusal: handing
    /// it the framed sentence puts `refused:` in front of a refusal, which is
    /// the doubled prefix this type exists to make unrepresentable.
    #[test]
    fn the_agents_half_is_the_clause_and_the_persons_half_is_the_sentence() {
        let said = Utterance::refused(&"\"P-01\" is already taken");
        let person = said.sentence();
        let agent = said.into_clause();
        assert_eq!(agent, "\"P-01\" is already taken");
        assert_eq!(person, format!("refused: {agent}"));
        assert!(!agent.contains("refused"), "the agent's half is not framed");
    }

    #[test]
    fn the_clause_is_stored_trimmed_so_two_spellings_are_one_utterance() {
        assert_eq!(
            Utterance::done("  R-01 moved  "),
            Utterance::done("R-01 moved")
        );
    }

    /// ★★★★★ R1718's gate, over the type this module puts in front of a
    /// **person** — and the check that refused this module's first draft.
    ///
    /// One clause, every tone: what varies is the frame alone, so this asks
    /// exactly the question that matters — can a reader who hears the sentence
    /// tell which situation produced it. Two tones sharing no frame answered
    /// identically, and since those two also share an urgency, nothing reached
    /// a reader who cannot see the toast's colour.
    #[test]
    fn every_tone_says_the_same_clause_differently() {
        use crate::test_fixtures::speech::assert_speaks;

        const CLAUSE: &str = "R-01 is where it was";
        let said: Vec<(&str, String)> = Tone::ALL
            .iter()
            .zip(Tone::ARM_NAMES)
            .map(|(tone, name)| (name, Utterance::new(*tone, CLAUSE).sentence()))
            .collect();
        assert_speaks("Utterance", Tone::ARMS, &said, &[]);
    }

    /// ★★★★★ R1778 — what [`Saying`] puts in front of a person, both ways.
    ///
    /// Demanded by this crate's own speech census the moment the type existed,
    /// and the demand is right even though `Saying` composes no wording of its
    /// own: it has two arms a reader meets — a sentence is current, or nothing
    /// is — and the SILENT one is the arm that could rot unnoticed. If expiry
    /// ever left the last sentence behind, or answered some word for emptiness,
    /// only this says so.
    #[test]
    fn what_a_saying_puts_in_front_of_a_person_is_driven_both_ways() {
        use crate::animation::Tickable;
        use crate::test_fixtures::speech::assert_speaks;

        let owner = crate::reactive::Owner::new();
        owner.run(|| {
            let holder = Saying::new(2.6);
            let quiet = holder.sentence();
            holder.say(Utterance::done("R-01 is where it was"));
            let current = holder.sentence();
            for _ in 0..200 {
                holder.tick(1.0 / 60.0);
            }
            let after = holder.sentence();

            assert_eq!(
                quiet, after,
                "★ silence before anything was said and silence after it \
                 expired are the SAME arm — a holder that answered differently \
                 would be remembering what it stopped saying",
            );
            let said: Vec<(&str, String)> = vec![("current", current), ("silent", after)];
            assert_speaks("Saying", 2, &said, &["silent"]);
        });
    }

    /// ★★★★★ R1778 — a sentence is current, and then it is not.
    ///
    /// Driven in the steps a paint loop actually takes rather than in one jump:
    /// a lifetime that only expires when handed its whole duration at once is
    /// not one a running application would ever reach.
    #[test]
    fn what_is_said_stops_being_said_when_its_time_is_up() {
        use crate::animation::Tickable;

        let owner = crate::reactive::Owner::new();
        owner.run(|| {
            let said = Saying::new(2.6);
            assert!(said.showing().is_none(), "nothing has been said yet");
            assert_eq!(said.sentence(), "");

            said.say(Utterance::done("a thing happened"));
            assert!(said.showing().is_some());
            assert_eq!(said.sentence(), "a thing happened");
            assert!((said.left() - 2.6).abs() < 1e-6);

            for _ in 0..60 {
                said.tick(1.0 / 60.0);
            }
            assert!(
                said.showing().is_some(),
                "one second in, a 2.6s sentence is still current",
            );

            for _ in 0..120 {
                said.tick(1.0 / 60.0);
            }
            assert!(said.showing().is_none(), "past its life it is gone");
            assert_eq!(said.sentence(), "");
            assert!(said.left().abs() < f32::EPSILON);
        });
    }

    /// Saying again restarts the life rather than queueing behind it.
    #[test]
    fn a_second_sentence_replaces_the_first_and_gets_a_full_life() {
        use crate::animation::Tickable;

        let owner = crate::reactive::Owner::new();
        owner.run(|| {
            let said = Saying::new(2.6);
            said.say(Utterance::done("first"));
            for _ in 0..120 {
                said.tick(1.0 / 60.0);
            }
            said.say(Utterance::done("second"));
            assert_eq!(said.sentence(), "second");
            assert!(
                (said.left() - 2.6).abs() < 1e-6,
                "the second sentence gets a whole life, not what the first left",
            );
        });
    }

    /// ★★ The clause a driver reads to decide whether to keep waking this.
    ///
    /// Both directions, because `is_at_rest` returning `true` while a sentence
    /// is current is exactly how a message would come to stay up forever — the
    /// defect this type was built for — and it would look like a working
    /// lifetime in every other test.
    #[test]
    fn a_current_sentence_is_not_at_rest_and_an_expired_one_is() {
        use crate::animation::Tickable;

        let owner = crate::reactive::Owner::new();
        owner.run(|| {
            let said = Saying::new(2.6);
            assert!(said.is_at_rest(0.001), "silence is at rest");
            said.say(Utterance::done("something"));
            assert!(
                !said.is_at_rest(0.001),
                "a sentence waiting to expire is work, even with nothing moving",
            );
            for _ in 0..200 {
                said.tick(1.0 / 60.0);
            }
            assert!(
                said.is_at_rest(0.001),
                "once gone there is nothing to count"
            );
        });
    }

    /// A ticker that is never told anything must not count down from nothing.
    #[test]
    fn ticking_while_silent_changes_nothing() {
        use crate::animation::Tickable;

        let owner = crate::reactive::Owner::new();
        owner.run(|| {
            let said = Saying::new(2.6);
            for _ in 0..200 {
                said.tick(1.0 / 60.0);
            }
            assert!(said.showing().is_none());
            assert!(
                said.left().abs() < f32::EPSILON,
                "it never had a life to spend"
            );
        });
    }

    /// R1718's gate, over the type this module puts in front of a developer.
    ///
    /// `Fault` is a clause producer — its caller supplies the subject (the
    /// clause that could not be said) — so it is driven with the subject named,
    /// and no arm may repeat it.
    #[test]
    fn every_fault_says_its_own_clause_about_a_clause_it_does_not_name() {
        use crate::test_fixtures::speech::assert_speaks_of;

        const CLAUSE: &str = "refused: taken";
        let said: Vec<(&str, String)> = Fault::ALL
            .iter()
            .zip(Fault::ARM_NAMES)
            .map(|(fault, name)| (name, fault.sentence()))
            .collect();
        assert_speaks_of("Fault", CLAUSE, Fault::ARMS, &said, &[]);
    }
}
