//! R1668 §5.39 §5.40 §2 #7 — **why a region is not available**, as a value.
//!
//! ## What the disabled cascade could not say
//!
//! R1554 gave [`LayoutStyle`](crate::style::LayoutStyle) the one inherited
//! interaction property — a region declares itself unavailable and its whole
//! subtree becomes inert, faded and announced so — and gave the census the
//! half no comparable toolkit has: **which** ancestor declared it. What neither
//! it nor the toolkit it was measured against could say is **why**.
//!
//! Measured by building and running the reference toolkit at 6.11: the
//! declaration is a bool on a widget, a bool on an action, a bool on a quick
//! item and one flag bit on a model item; assistive technology receives one bit
//! of fifty-one, whose own source comment records that it used to be called
//! *unavailable*; and the only slots that could carry a reason are free-form
//! prose (`toolTip` / `whatsThis` / a description string) which nothing
//! classifies and nothing links to the disabled state. On an action that slot
//! is worse than empty — it defaults to the action's own label, so an author
//! who writes a reason into it loses the name.
//!
//! Four questions therefore have no answer there, and the fourth is the one a
//! screen most needs: **a feature reserved for a release that has not shipped
//! and a feature this build will never have are the same bool.**
//!
//! ## The shape
//!
//! A declaration carries an [`Unavailable`]: a [`kind`](Unavailable::kind) from
//! a closed vocabulary, and one [`detail`](Unavailable::detail) whose meaning
//! the kind fixes. One slot rather than several is the same choice
//! [`CardState::Denied`](crate::widgets::card::CardState::Denied) already
//! makes — the arm says how to read the string, so the string never has to be
//! parsed to find out what it is.
//!
//! The arms are justified the way this codebase justifies every closed
//! vocabulary: **each one leaves a person with a different thing to do**, and
//! that difference is itself derived, once, as a [`Recourse`]. A screen that
//! states a recourse per site would be authoring the same table again at every
//! site, and the sites would disagree.
//!
//! ```
//! use pinion_core::availability::{Recourse, Unavailable};
//!
//! let reserved = Unavailable::reserved("the second release");
//! assert_eq!(reserved.recourse(), Recourse::AwaitRelease);
//! assert_eq!(reserved.detail(), "the second release");
//!
//! // Not the same answer, and not the same bool either.
//! let never = Unavailable::unsupported("this platform has no such device");
//! assert_eq!(never.recourse(), Recourse::Nothing);
//! ```
//!
//! ## Where it goes
//!
//! The reason travels with the cascade rather than beside it: every node the
//! cascade resolves as unavailable carries the **declaring** node's reason, so
//! a control deep inside a reserved panel answers the question without a walk.
//! It reaches the wire on `scene/disabled` and the accessibility tree on
//! [`AccessNode`](../../pinion_a11y/struct.AccessNode.html), which is what makes
//! the difference between not-yet and never audible rather than merely present
//! in a struct.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// What class of unavailability a region declares.
///
/// Closed, and each arm exists because it leaves a person with a different
/// thing to do — see [`Recourse`], which is that difference derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableKind {
    /// A condition of the current session is not met, and meeting it would
    /// make this available. [`detail`](Unavailable::detail) names the
    /// condition.
    Precondition,
    /// The viewer's authority does not extend here.
    /// [`detail`](Unavailable::detail) names the authority that would.
    Permission,
    /// Something holds it right now and will let go.
    /// [`detail`](Unavailable::detail) names the holder.
    Busy,
    /// It is named, described and booked for work that has not shipped — the
    /// state the reference analysis tool's screen C puts nine of its widgets
    /// in, deliberately visible rather than hidden, so that the shape of the
    /// finished product is legible before it exists.
    /// [`detail`](Unavailable::detail) names the release or the requirement it
    /// is booked under.
    Reserved,
    /// This build or platform does not offer it and no action changes that.
    /// [`detail`](Unavailable::detail) names what is missing.
    ///
    /// The arm [`Reserved`](Self::Reserved) is measured against: both are inert
    /// today, and only one will ever stop being.
    Unsupported,
    /// ★★ R1695 — the product has it and **this surface** does not.
    /// [`detail`](Unavailable::detail) names the surface that does.
    ///
    /// Measured on the analysis tool: its navigation rail offers seven
    /// destinations and the application behind the rail hosts one of them, so
    /// four seats claimed an arrival that never happened. Neither existing arm
    /// says that. [`Reserved`](Self::Reserved) is false — the thing is built and
    /// shipping — and [`Unsupported`](Self::Unsupported) is false too, because
    /// its recourse is [`Nothing`](Recourse::Nothing) while here there is a
    /// perfectly good something: go to the surface that has it. A reader told
    /// "not available in this build" would stop looking for a feature that is
    /// one window away.
    ///
    /// General beyond that screen, and the reason to spend an arm on it: any
    /// product with more than one surface routes work between them, and
    /// "editable in the other editor" is the sentence it needs. The rule this
    /// module states for adding an arm is met — **the reader's next action is
    /// its own**, which is why it derives its own [`Recourse`].
    Elsewhere,
    /// A region was declared unavailable and no reason was given.
    ///
    /// Deliberately an arm rather than an absence. Every declaration that
    /// predates this module is in this state, and an arm makes them countable:
    /// "how many inert regions cannot say why" is a number a census reports,
    /// where an `Option` would have made it a silence.
    Unstated,
}

impl UnavailableKind {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            UnavailableKind::Precondition => "precondition",
            UnavailableKind::Permission => "permission",
            UnavailableKind::Busy => "busy",
            UnavailableKind::Reserved => "reserved",
            UnavailableKind::Unsupported => "unsupported",
            UnavailableKind::Elsewhere => "elsewhere",
            UnavailableKind::Unstated => "unstated",
        }
    }

    /// Parse a wire spelling back.
    ///
    /// The inverse of [`name`](Self::name), so what a surface publishes is what
    /// it accepts — the symmetry R1616 made a rule after a published vocabulary
    /// turned out not to be a readable one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == name)
    }

    /// Every arm, in declaration order.
    ///
    /// A member list rather than a count: R1650 measured that proving a set is
    /// complete by searching for what is missing yields zero every time.
    pub const ALL: [UnavailableKind; 7] = [
        UnavailableKind::Precondition,
        UnavailableKind::Permission,
        UnavailableKind::Busy,
        UnavailableKind::Reserved,
        UnavailableKind::Unsupported,
        UnavailableKind::Elsewhere,
        UnavailableKind::Unstated,
    ];

    /// What a person can do about it.
    ///
    /// Derived here, once, rather than restated at each site — the same posture
    /// [`Remedy`](crate::widgets::card::Remedy) takes for a card's content
    /// state. It is a **separate** vocabulary from `Remedy` for a measured
    /// reason: a card's remedy set has no arm for *satisfy the stated
    /// condition*, because a card body that is empty cannot be filled by the
    /// reader, and an affordance that is waiting on a precondition can.
    #[must_use]
    pub const fn recourse(self) -> Recourse {
        match self {
            UnavailableKind::Precondition => Recourse::Satisfy,
            UnavailableKind::Permission => Recourse::Authorize,
            UnavailableKind::Busy => Recourse::Wait,
            UnavailableKind::Reserved => Recourse::AwaitRelease,
            UnavailableKind::Elsewhere => Recourse::OpenElsewhere,
            // Nothing today and nothing later — but for different reasons, which
            // is why the kinds stay apart even though the recourse merges.
            UnavailableKind::Unsupported | UnavailableKind::Unstated => Recourse::Nothing,
        }
    }
}

/// What a person can do about an unavailable region — derived from
/// [`UnavailableKind`], never declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recourse {
    /// Meet the stated condition; it is within reach of this session.
    Satisfy,
    /// Obtain the authority from somebody who has it.
    Authorize,
    /// Wait for the current holder to finish. Short.
    Wait,
    /// Wait for a release. Nothing done today changes it, and it will arrive.
    AwaitRelease,
    /// ★ R1695 — go to the surface that has it; it is built and it is not here.
    ///
    /// Available today, like [`Satisfy`](Self::Satisfy), and out of this
    /// surface's hands, like [`Nothing`](Self::Nothing) — which is why it is
    /// neither. The action is a **move**, and a reader who is told to wait or
    /// to give up will not make it.
    OpenElsewhere,
    /// Nothing, in this build.
    Nothing,
}

impl Recourse {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Recourse::Satisfy => "satisfy",
            Recourse::Authorize => "authorize",
            Recourse::Wait => "wait",
            Recourse::AwaitRelease => "await_release",
            Recourse::OpenElsewhere => "open_elsewhere",
            Recourse::Nothing => "nothing",
        }
    }

    /// Parse a wire spelling back.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.name() == name)
    }

    /// Every arm, in declaration order.
    pub const ALL: [Recourse; 6] = [
        Recourse::Satisfy,
        Recourse::Authorize,
        Recourse::Wait,
        Recourse::AwaitRelease,
        Recourse::OpenElsewhere,
        Recourse::Nothing,
    ];

    /// Whether the region will become available on its own, without the viewer
    /// doing anything and without a different build.
    ///
    /// The one predicate a shell needs that neither the kind nor the recourse
    /// answers alone: it separates *wait* from *ask* from *give up*.
    #[must_use]
    pub const fn resolves_by_itself(self) -> bool {
        matches!(self, Recourse::Wait | Recourse::AwaitRelease)
    }
}

/// Every wire spelling an [`UnavailableKind`] can take, derived from the arms
/// so a published vocabulary cannot lag the enum.
///
/// A client's claim on this axis is that it can match the kind **exhaustively**,
/// and a hand-written list would make that claim wrong at the moment a seventh
/// arm arrived — which is the failure R1616 made a rule after.
pub const KIND_WIRE_NAMES: [&str; UnavailableKind::ALL.len()] = {
    let mut names = [""; UnavailableKind::ALL.len()];
    let mut i = 0;
    while i < UnavailableKind::ALL.len() {
        names[i] = UnavailableKind::ALL[i].name();
        i += 1;
    }
    names
};

/// Every wire spelling a [`Recourse`] can take, derived the same way.
pub const RECOURSE_WIRE_NAMES: [&str; Recourse::ALL.len()] = {
    let mut names = [""; Recourse::ALL.len()];
    let mut i = 0;
    while i < Recourse::ALL.len() {
        names[i] = Recourse::ALL[i].name();
        i += 1;
    }
    names
};

/// Why a region is not available.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Unavailable {
    kind: UnavailableKind,
    detail: Cow<'static, str>,
}

impl Unavailable {
    /// A reason of the given kind, whose detail the kind's documentation fixes
    /// the meaning of.
    #[must_use]
    pub fn new(kind: UnavailableKind, detail: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// A condition of this session is not met; `detail` names the condition.
    #[must_use]
    pub fn precondition(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(UnavailableKind::Precondition, detail)
    }

    /// The viewer's authority does not extend here; `detail` names what would.
    #[must_use]
    pub fn permission(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(UnavailableKind::Permission, detail)
    }

    /// Something holds it; `detail` names the holder.
    #[must_use]
    pub fn busy(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(UnavailableKind::Busy, detail)
    }

    /// Booked for work that has not shipped; `detail` names the release or
    /// requirement it is booked under.
    #[must_use]
    pub fn reserved(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(UnavailableKind::Reserved, detail)
    }

    /// Absent from this build or platform for good; `detail` names what is
    /// missing.
    #[must_use]
    pub fn unsupported(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(UnavailableKind::Unsupported, detail)
    }

    /// ★ R1695 — built and shipping, on a different surface of this product;
    /// `detail` names that surface.
    #[must_use]
    pub fn elsewhere(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(UnavailableKind::Elsewhere, detail)
    }

    /// Declared unavailable with no reason given — what
    /// [`with_disabled(true)`](crate::style::LayoutStyle::with_disabled)
    /// produces, and what every declaration written before this module means.
    #[must_use]
    pub const fn unstated() -> Self {
        Self {
            kind: UnavailableKind::Unstated,
            detail: Cow::Borrowed(""),
        }
    }

    /// The class.
    #[must_use]
    pub const fn kind(&self) -> UnavailableKind {
        self.kind
    }

    /// The specific thing the class points at.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// What a person can do about it — derived from [`kind`](Self::kind).
    #[must_use]
    pub const fn recourse(&self) -> Recourse {
        self.kind.recourse()
    }

    /// The reason as one phrase, for a listener.
    ///
    /// Derived from [`kind`](Self::kind) and [`detail`](Self::detail) so a
    /// screen never authors it — and so the accessibility tree and the wire
    /// cannot describe the same region differently. It lowers to the
    /// accessibility layer's **state description**, which replaces the default
    /// announcement for a disabled node; the reference toolkit at 6.11 has one
    /// bit there and nothing to put a phrase in.
    ///
    /// A kind whose detail is empty renders the kind alone rather than a
    /// dangling preposition.
    ///
    /// ```
    /// use pinion_core::availability::Unavailable;
    ///
    /// assert_eq!(
    ///     Unavailable::reserved("the second release").sentence(),
    ///     "reserved for the second release",
    /// );
    /// assert_eq!(Unavailable::unstated().sentence(), "unavailable");
    /// ```
    #[must_use]
    pub fn sentence(&self) -> String {
        let detail = self.detail();
        if detail.is_empty() {
            return match self.kind {
                UnavailableKind::Precondition => "unavailable until a condition is met",
                UnavailableKind::Permission => "not permitted",
                UnavailableKind::Busy => "in use",
                UnavailableKind::Reserved => "reserved for a later release",
                UnavailableKind::Unsupported => "not available in this build",
                UnavailableKind::Elsewhere => "on another surface of this product",
                UnavailableKind::Unstated => "unavailable",
            }
            .to_owned();
        }
        let lead = match self.kind {
            UnavailableKind::Precondition => "unavailable until",
            UnavailableKind::Permission => "requires",
            UnavailableKind::Busy => "in use by",
            UnavailableKind::Reserved => "reserved for",
            UnavailableKind::Unsupported => "not available in this build:",
            UnavailableKind::Elsewhere => "in",
            UnavailableKind::Unstated => "unavailable:",
        };
        format!("{lead} {detail}")
    }

    /// Whether the author said anything at all.
    ///
    /// The predicate a census counts with. It is deliberately not "is the
    /// detail empty": a stated kind with an empty detail still says more than
    /// a bool did.
    #[must_use]
    pub const fn is_stated(&self) -> bool {
        !matches!(self.kind, UnavailableKind::Unstated)
    }
}

#[cfg(test)]
mod tests {
    use super::{Recourse, Unavailable, UnavailableKind};

    /// R1668 — every kind names a recourse, and the wire spelling round-trips.
    ///
    /// Total over the vocabulary by construction: the population is
    /// [`UnavailableKind::ALL`], so an arm added without a wire name or a
    /// recourse fails here rather than in whatever screen reached for it.
    #[test]
    fn r1668_every_kind_names_a_recourse_and_round_trips() {
        let mut seen_names = std::collections::BTreeSet::new();
        for kind in UnavailableKind::ALL {
            assert!(
                seen_names.insert(kind.name()),
                "{kind:?} shares a wire name with another kind"
            );
            assert_eq!(
                UnavailableKind::from_name(kind.name()),
                Some(kind),
                "{kind:?} publishes a name its own reader does not accept"
            );
            let recourse = kind.recourse();
            assert_eq!(
                Recourse::from_name(recourse.name()),
                Some(recourse),
                "{recourse:?} publishes a name its own reader does not accept"
            );
        }
        assert_eq!(UnavailableKind::ALL.len(), 7);
        assert_eq!(Recourse::ALL.len(), 6);
    }

    /// ★★ R1695 — **available today, and not from here** is its own answer.
    ///
    /// The three inert-today arms are kept apart by what the reader should do
    /// next, which is the rule this module states for spending an arm. A tool
    /// that collapsed them would send a reader to wait for a release that has
    /// already shipped, or to give up on a feature one window away.
    #[test]
    fn r1695_elsewhere_is_neither_waiting_nor_giving_up() {
        let elsewhere = Unavailable::elsewhere("the packet viewer");
        assert_eq!(elsewhere.recourse(), Recourse::OpenElsewhere);
        assert_eq!(elsewhere.sentence(), "in the packet viewer");
        assert!(elsewhere.is_stated());

        // Not waiting: nothing arrives on its own, because it has arrived.
        assert!(!elsewhere.recourse().resolves_by_itself());
        assert_ne!(
            elsewhere.recourse(),
            Unavailable::reserved("r12").recourse()
        );
        // Not giving up either, which is the confusion that costs a reader the
        // feature: `unsupported` says no action helps, and here one does.
        assert_ne!(
            elsewhere.recourse(),
            Unavailable::unsupported("no such device").recourse()
        );

        // The recourse is this arm's alone, so a client switching on the
        // recourse can act on it without also reading the kind.
        let sharing: Vec<_> = UnavailableKind::ALL
            .iter()
            .filter(|k| k.recourse() == Recourse::OpenElsewhere)
            .collect();
        assert_eq!(sharing, vec![&UnavailableKind::Elsewhere]);
    }

    /// R1668 — the arms exist because the recourses differ, and the two that
    /// share one are kept apart by the question a shell actually asks.
    ///
    /// The floor this is measured against collapses all six into one bool, and
    /// the collapse that costs the most is reserved-versus-unsupported: both
    /// are inert today and only one will ever stop being.
    #[test]
    fn r1668_reserved_and_unsupported_are_not_the_same_answer() {
        let reserved = Unavailable::reserved("the second release");
        let never = Unavailable::unsupported("no such device on this platform");

        assert_eq!(reserved.recourse(), Recourse::AwaitRelease);
        assert_eq!(never.recourse(), Recourse::Nothing);
        assert!(reserved.recourse().resolves_by_itself());
        assert!(!never.recourse().resolves_by_itself());

        // And the recourse partition is not the identity on kinds: two kinds
        // share `Nothing`, so a consumer that switched on the recourse alone
        // would be unable to tell them apart -- which is why the kind is
        // published too.
        let sharing: Vec<_> = UnavailableKind::ALL
            .iter()
            .filter(|k| k.recourse() == Recourse::Nothing)
            .collect();
        assert_eq!(sharing.len(), 2, "unsupported and unstated");
    }

    /// R1668 — the recourses that resolve by themselves are exactly the two
    /// that name waiting, and they name different waits.
    #[test]
    fn r1668_waiting_for_a_holder_is_not_waiting_for_a_release() {
        let busy = Unavailable::busy("the export running in this window");
        let reserved = Unavailable::reserved("requirement 16");
        assert!(busy.recourse().resolves_by_itself());
        assert!(reserved.recourse().resolves_by_itself());
        assert_ne!(busy.recourse(), reserved.recourse());

        let resolving: Vec<_> = Recourse::ALL
            .iter()
            .filter(|r| r.resolves_by_itself())
            .copied()
            .collect();
        assert_eq!(resolving, vec![Recourse::Wait, Recourse::AwaitRelease]);
    }

    /// R1668 — an unstated reason is a countable arm, not a silence.
    #[test]
    fn r1668_unstated_is_an_arm_a_census_can_count() {
        let quiet = Unavailable::unstated();
        assert!(!quiet.is_stated());
        assert_eq!(quiet.kind(), UnavailableKind::Unstated);
        assert_eq!(quiet.detail(), "");

        for kind in UnavailableKind::ALL {
            let stated = Unavailable::new(kind, "something");
            assert_eq!(
                stated.is_stated(),
                kind != UnavailableKind::Unstated,
                "{kind:?} disagrees with its own statedness"
            );
        }
    }

    /// ★★★★★ R1718 — **every reason a person can be given for a shut
    /// affordance is said, and no two of them read alike.**
    ///
    /// This one reaches a screen reader: an accessibility node's state
    /// description is exactly this sentence, so an arm that read like another
    /// would tell somebody who cannot see the seat the wrong thing about why
    /// they cannot use it — and the census that existed counted the KINDS, not
    /// what they say. Both halves are driven, because a kind with a detail and
    /// the same kind without one are two sentences a person meets.
    #[test]
    fn r1718_every_reason_a_shut_affordance_gives_is_said_and_distinct() {
        use crate::test_fixtures::speech::assert_speaks;

        let bare: Vec<(&str, String)> = UnavailableKind::ALL
            .iter()
            .map(|kind| (kind.name(), Unavailable::new(*kind, "").sentence()))
            .collect();
        assert_speaks(
            "Unavailable (no detail)",
            UnavailableKind::ALL.len(),
            &bare,
            &[],
        );

        let detailed: Vec<(&str, String)> = UnavailableKind::ALL
            .iter()
            .map(|kind| {
                (
                    kind.name(),
                    Unavailable::new(*kind, "the second release").sentence(),
                )
            })
            .collect();
        assert_speaks(
            "Unavailable (with detail)",
            UnavailableKind::ALL.len(),
            &detailed,
            &[],
        );
    }
}
