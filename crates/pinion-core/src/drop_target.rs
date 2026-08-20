//! R1734 §5.51 §5.15 — the **target** side of a drag.
//!
//! Until this module, a drag session in this tree spoke to exactly one party:
//! the surface that started it. [`External::begin_drag`] opened a session, and
//! `drag_to` / `drag_release` / `drag_cancel` all went back to that same
//! surface, which had to hit-test the world from the [`DropPoint`] the router
//! handed it and decide, on the destination's behalf, what a release would
//! mean. That works exactly when one coordinator owns both ends — every
//! in-tree consumer so far (a reorder list, a tab strip, a dock, a tree
//! reparent) is that shape — and it cannot express a drop **between two
//! surfaces** at all, because the destination is never asked anything.
//!
//! [`External::begin_drag`]: crate::external::External::begin_drag
//! [`DropPoint`]: crate::external::DropPoint
//!
//! # Where the floor stands, measured
//!
//! A mature retained-mode toolkit is **above** this tree on the plain
//! question. Driven offscreen against its 6.11.1 release, a target widget
//! receives an enter, three moves and a drop; each event carries the payload's
//! format list, the proposed action and the set of possible actions; and the
//! target accepted a payload in a format it had never heard of, from a source
//! that had never heard of *it*. So "the destination is asked" is not a
//! frontier — it is table stakes this tree owed.
//!
//! The same probe measured where that floor stops, and it stops in three
//! places this module is built around:
//!
//! 1. **You cannot ask before the drag.** Acceptance is one boolean per
//!    widget, and the decision that matters lives *inside* an event handler
//!    you must run — so "where could this land?" is unanswerable until
//!    something is already in flight. Per-part acceptance is a second boolean
//!    (a row is drop-enabled or not) and names no kind at all: a part can say
//!    YES or NO and cannot say WHAT.
//! 2. **A refusal carries no reason.** The accept-or-ignore call is a bare
//!    bool; a client that is refused learns nothing about what would have
//!    worked.
//! 3. **The preview and the commit are two computations.** The move event
//!    carries a pixel. Nothing on the event, the widget or the layout turns
//!    that pixel into the cell a release would use, so the highlight a target
//!    draws and the outcome it later commits are free to disagree — the exact
//!    class R1668 paid for here and R1733 answered structurally by giving a
//!    grid carry ONE landing that both the preview and the commit read.
//!
//! # And the third one is not hypothetical — it is in the behaviour reference
//!
//! Measured in the prototype this workspace reproduces, rather than asserted
//! about a toolkit. Its board is the drop target, and its two handlers each
//! compute the destination cell from the cursor: the one that runs while the
//! drag is over it does so to draw the snap mark, and the one that runs on
//! release does so *again* to decide where to add. One fact, computed twice,
//! from two different events, with nothing holding them to each other. Its
//! palette declares the drag a **copy** at drag start, which is where this
//! module's `copy` comes from rather than from anybody's preference. And its
//! board raises the "yes, here" highlight even for a payload kind it does not
//! recognise, because acceptance is decided inside the handler that is already
//! drawing — a declaration that gates dispatch cannot do that, since the
//! refusal happens before any preview exists.
//!
//! # What this module does instead
//!
//! * A surface **declares** what it takes, as data, ahead of any drag
//!   ([`DropContract`]), and that declaration is published on the same
//!   introspection channel as everything else, so an agent can ask *before*
//!   picking anything up. This is not reachable from the floor's design: its
//!   accept predicate is code inside a handler.
//! * The declaration is a **precondition of dispatch**. The router derives the
//!   structural refusals from it — wrong kind, wrong part, no action in common
//!   — and never asks a surface about a drag its own declaration excludes.
//!   That is the same rule this workspace already applies to actions, and it
//!   is what makes "declared but inert" and "undeclared but live" both
//!   unrepresentable rather than merely discouraged.
//! * A refusal **states what would have worked** ([`DropRefusal`]), in the
//!   shape [`ReadRefusal`](crate::external::ReadRefusal) already uses on the
//!   read channel.
//! * An acceptance **carries the landing it previewed** ([`DropAccept`]), and
//!   the commit takes that acceptance as its witness rather than recomputing.
//!   A target therefore cannot commit somewhere it did not preview — the
//!   R1733 shape, lifted from one grid widget to the input contract.

use crate::external::{DragPayload, DropPoint, IntrospectValue, RefusalReason};
use crate::input::Modifiers;

/// What a drop would DO with the thing being dragged.
///
/// Three arms because three are what the platform drag protocols agree on, and
/// because the difference is observable to the person: a copy leaves the source
/// alone, a move empties it, a link leaves both and relates them. A source
/// offers the set it can honour ([`DragPayload::actions`]); a target picks
/// exactly one ([`DropAccept::action`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropAction {
    /// The source keeps what it had; the target gains a duplicate.
    Copy,
    /// The source gives up what it had; the target gains it.
    Move,
    /// Both keep what they had, related.
    Link,
}

impl DropAction {
    /// Every arm, in the order a set iterates and a sentence lists them.
    pub const ALL: [Self; 3] = [Self::Copy, Self::Move, Self::Link];

    /// The one-bit membership mask this arm occupies in a [`DropActions`] set.
    const fn bit(self) -> u8 {
        match self {
            Self::Copy => 1,
            Self::Move => 2,
            Self::Link => 4,
        }
    }

    /// The wire spelling — what `$drop` publishes and what an agent matches on.
    ///
    /// ```
    /// # use pinion_core::drop_target::DropAction;
    /// assert_eq!(DropAction::Move.as_wire_name(), "move");
    /// ```
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Link => "link",
        }
    }

    /// The inverse of [`as_wire_name`](Self::as_wire_name) — `None` for a word
    /// this vocabulary does not have.
    ///
    /// The pair is round-trip tested rather than eyeballed, because a wire
    /// vocabulary written twice is a vocabulary that drifts once.
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_wire_name() == name)
    }
}

/// A **non-empty** set of [`DropAction`]s.
///
/// Non-empty *by construction*: the only ways to obtain one are
/// [`one`](Self::one) (which names an arm) and [`with`](Self::with) (which adds
/// one), and [`intersect`](Self::intersect) answers `Option` rather than
/// handing back an empty set. So "this surface accepts the kind but can do
/// nothing with it" is not a value anybody can build — it is a refusal, and it
/// is [`DropRefusal::NoCommonAction`].
///
/// ```
/// # use pinion_core::drop_target::{DropAction, DropActions};
/// let source = DropActions::one(DropAction::Copy).with(DropAction::Move);
/// let target = DropActions::one(DropAction::Move);
/// assert_eq!(source.intersect(target), Some(target));
/// assert_eq!(DropActions::one(DropAction::Link).intersect(target), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DropActions(u8);

impl DropActions {
    /// The set holding exactly `action`.
    #[must_use]
    pub const fn one(action: DropAction) -> Self {
        Self(action.bit())
    }

    /// This set, plus `action`.
    #[must_use]
    pub const fn with(self, action: DropAction) -> Self {
        Self(self.0 | action.bit())
    }

    /// Is `action` a member?
    #[must_use]
    pub const fn contains(self, action: DropAction) -> bool {
        self.0 & action.bit() != 0
    }

    /// How many arms are members. Never zero.
    ///
    /// `count` rather than `len`, deliberately: a `len` invites an `is_empty`
    /// beside it, and an `is_empty` on this type would be a method that always
    /// answers `false` — a question the type exists to make unaskable, dressed
    /// up as one a caller should ask.
    #[must_use]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// Members in [`DropAction::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = DropAction> {
        DropAction::ALL
            .into_iter()
            .filter(move |a| self.contains(*a))
    }

    /// The members common to both sets, or `None` when they share none.
    ///
    /// `Option` rather than a possibly-empty set is the whole invariant: a
    /// caller cannot forget to check, because there is nothing to unwrap until
    /// it has.
    #[must_use]
    pub fn intersect(self, other: Self) -> Option<Self> {
        let common = self.0 & other.0;
        (common != 0).then_some(Self(common))
    }

    /// The member a target lands on when it expresses no preference: the first
    /// in [`DropAction::ALL`] order.
    ///
    /// Total rather than an `Option` — every constructible set has a member —
    /// and written as an exhaustive ladder rather than `iter().next().unwrap()`
    /// so the totality is visible in the code instead of asserted in a comment.
    #[must_use]
    pub const fn first(self) -> DropAction {
        if self.contains(DropAction::Copy) {
            DropAction::Copy
        } else if self.contains(DropAction::Move) {
            DropAction::Move
        } else {
            DropAction::Link
        }
    }

    /// The members' wire spellings, in set order — what `$drop` renders.
    #[must_use]
    pub fn wire_names(self) -> Vec<&'static str> {
        self.iter().map(DropAction::as_wire_name).collect()
    }
}

/// WHICH part of a surface a [`DropClause`] applies to.
///
/// The floor's per-part acceptance is one boolean and names no kind (measured:
/// a drop-enabled row says YES or NO and cannot say WHAT). Naming the parts
/// here is what lets one surface accept different kinds in different places —
/// a board that takes a widget footprint on its cells and a saved layout on its
/// header — and lets a client read that split off the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropRegion {
    /// Every part of the surface, including the bare surface tag itself.
    Surface,
    /// Only these composite sub-parts — the `#sub` half of a paint tag
    /// ([`split_subindex`](crate::composite_tag::split_subindex)). A drop over
    /// the bare surface tag is NOT admitted by this arm: the clause named the
    /// parts it wanted, and the surface as a whole is not one of them.
    Parts(&'static [&'static str]),
}

/// One line of a [`DropContract`]: a payload kind, what this surface can do
/// with it, and where on this surface it may land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropClause {
    /// The [`DragPayload::kind`] this clause admits.
    pub kind: &'static str,
    /// What this surface can do with that kind. Non-empty by construction.
    pub actions: DropActions,
    /// Where on this surface the clause applies.
    pub region: DropRegion,
}

impl DropClause {
    /// A clause covering the whole surface.
    #[must_use]
    pub const fn surface(kind: &'static str, actions: DropActions) -> Self {
        Self {
            kind,
            actions,
            region: DropRegion::Surface,
        }
    }

    /// A clause covering only the named composite sub-parts.
    #[must_use]
    pub const fn parts(
        kind: &'static str,
        actions: DropActions,
        parts: &'static [&'static str],
    ) -> Self {
        Self {
            kind,
            actions,
            region: DropRegion::Parts(parts),
        }
    }

    /// Does this clause cover `part` (the `#sub` half of the drop point's tag,
    /// `None` when the cursor is over the bare surface tag)?
    #[must_use]
    pub fn admits_part(&self, part: Option<&str>) -> bool {
        match self.region {
            DropRegion::Surface => true,
            DropRegion::Parts(names) => part.is_some_and(|p| names.contains(&p)),
        }
    }

    /// The parts this clause names, for a refusal that must say what would have
    /// worked. Empty for a whole-surface clause, which names none.
    #[must_use]
    pub const fn named_parts(&self) -> &'static [&'static str] {
        match self.region {
            DropRegion::Surface => &[],
            DropRegion::Parts(names) => names,
        }
    }
}

/// What a surface accepts, **declared ahead of any drag**.
///
/// Static, exactly like [`IntrospectSchema`](crate::external::IntrospectSchema)
/// and for the same reason: the declaration is the surface's fixed contract,
/// while everything that depends on live state is the *verdict*'s business
/// ([`External::drop_offered`](crate::external::External::drop_offered)). A
/// board whose cells are full has not stopped accepting widget footprints; it
/// declines this particular offer, with a reason, and says so in a sentence a
/// person can read.
///
/// Being `Copy` and `'static` also means asking for it costs nothing, which
/// matters because the router asks on **every cursor sample** of a live drag.
///
/// ```
/// # use pinion_core::drop_target::{DropAction, DropActions, DropClause, DropContract};
/// const CONTRACT: DropContract = DropContract::new(const {
///     &[DropClause::surface(
///         "board-widget",
///         DropActions::one(DropAction::Copy).with(DropAction::Move),
///     )]
/// });
/// assert_eq!(CONTRACT.kinds().collect::<Vec<_>>(), ["board-widget"]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropContract {
    /// The clauses, in declared order. A refusal lists them in this order too,
    /// so what a client reads back matches what the surface wrote.
    pub clauses: &'static [DropClause],
}

impl DropContract {
    /// The contract of a surface that is not a drop target.
    ///
    /// This is the default, and it is load-bearing: the router dispatches
    /// nothing to a surface whose contract is empty, so every `External`
    /// written before this module is bit-identical under it.
    pub const EMPTY: Self = Self::new(&[]);

    /// A contract over `clauses`.
    #[must_use]
    pub const fn new(clauses: &'static [DropClause]) -> Self {
        Self { clauses }
    }

    /// Is this surface a drop target at all?
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// Every declared kind, in clause order, with duplicates kept — two clauses
    /// may name one kind for different parts, and collapsing them here would
    /// make a refusal's "what would have worked" list disagree with the
    /// declaration it was derived from.
    pub fn kinds(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.clauses.iter().map(|c| c.kind)
    }

    /// The framework's **derived** admissibility: may a payload of `kind`,
    /// offered with `offered` actions, land on `part` of this surface?
    ///
    /// `Ok` carries the actions the two sides have in common — the set the
    /// target then chooses from. `Err` states which of the three structural
    /// reasons applied and what would have worked instead.
    ///
    /// This is a pure function of the *published* declaration, which is the
    /// point: a client that has read `$drop` can run this reasoning itself,
    /// before it picks anything up. The floor cannot offer that at all — its
    /// accept predicate is code inside an event handler, so the only way to
    /// learn the answer is to already be dragging.
    ///
    /// # Errors
    ///
    /// [`DropRefusal::KindNotAccepted`] when no clause names `kind`;
    /// [`DropRefusal::PartNotAccepted`] when clauses name it but none covers
    /// `part`; [`DropRefusal::NoCommonAction`] when a covering clause exists
    /// and shares no action with `offered`.
    pub fn admits(
        &self,
        kind: &str,
        offered: DropActions,
        part: Option<&str>,
    ) -> Result<DropActions, DropRefusal> {
        let by_kind: Vec<&DropClause> = self.clauses.iter().filter(|c| c.kind == kind).collect();
        if by_kind.is_empty() {
            return Err(DropRefusal::KindNotAccepted {
                kind: kind.to_owned(),
                accepted: self.kinds().collect(),
            });
        }
        let covering: Vec<&DropClause> = by_kind
            .iter()
            .copied()
            .filter(|c| c.admits_part(part))
            .collect();
        if covering.is_empty() {
            return Err(DropRefusal::PartNotAccepted {
                part: part.map(ToOwned::to_owned),
                accepted: by_kind
                    .iter()
                    .flat_map(|c| c.named_parts())
                    .copied()
                    .collect(),
            });
        }
        let accepted = covering
            .iter()
            .map(|c| c.actions)
            .reduce(DropActions::with_all)
            .unwrap_or_else(|| DropActions::one(DropAction::Move));
        accepted
            .intersect(offered)
            .ok_or(DropRefusal::NoCommonAction { offered, accepted })
    }
}

impl DropActions {
    /// Every member of `self` plus every member of `other`.
    ///
    /// Only used to fold the clauses that cover one point into the surface's
    /// answer for that point, which is why it is not part of the public
    /// builder pair — [`with`](Self::with) is what a declaration writes.
    const fn with_all(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Why a drop was refused, **stating what would have worked**.
///
/// The floor answers this question with a bare boolean, so a refused client
/// learns only that it was refused. Three of these four arms are derived by the
/// framework from the surface's own published declaration and therefore cannot
/// drift from it; the fourth is the surface's own sentence about its live
/// state, which no declaration could have predicted.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum DropRefusal {
    /// No clause names this payload kind. Carries every kind that has one.
    KindNotAccepted {
        /// The kind that was offered.
        kind: String,
        /// The kinds this surface does declare, in clause order.
        accepted: Vec<&'static str>,
    },
    /// The kind is declared, and no clause covers the part under the cursor.
    /// Carries the parts those clauses do name.
    PartNotAccepted {
        /// The part under the cursor, `None` when it was the bare surface.
        part: Option<String>,
        /// The parts the kind's clauses name, in clause order.
        accepted: Vec<&'static str>,
    },
    /// A clause covers this point and shares no action with the source.
    NoCommonAction {
        /// What the source can do.
        offered: DropActions,
        /// What this surface can do here.
        accepted: DropActions,
    },
    /// The declaration admits this drop and the surface's live state does not.
    /// Carries the surface's own sentence.
    Declined(RefusalReason),
}

impl DropRefusal {
    /// Refuse for a reason only this surface's live state knows.
    ///
    /// ```
    /// # use pinion_core::drop_target::DropRefusal;
    /// let no = DropRefusal::declined("that cell is taken by the latency card");
    /// assert!(no.sentence().contains("latency card"));
    /// ```
    #[must_use]
    pub fn declined(reason: impl Into<RefusalReason>) -> Self {
        Self::Declined(reason.into())
    }

    /// The wire tag — one word an agent matches on, the peer of
    /// [`sentence`](Self::sentence).
    #[must_use]
    pub const fn as_wire_name(&self) -> &'static str {
        match self {
            Self::KindNotAccepted { .. } => "kind-not-accepted",
            Self::PartNotAccepted { .. } => "part-not-accepted",
            Self::NoCommonAction { .. } => "no-common-action",
            Self::Declined(_) => "declined",
        }
    }

    /// A sentence a **person** reads, naming what would have worked.
    ///
    /// Built from wire spellings and the surface's own words, never from a Rust
    /// value's debug form — the rule R1720 recorded after a refusal reached a
    /// person as constructor syntax.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::KindNotAccepted { kind, accepted } => {
                if accepted.is_empty() {
                    format!("nothing can be dropped here, and this is a {kind}")
                } else {
                    format!("this takes {}, and not a {kind}", join_words(accepted))
                }
            }
            Self::PartNotAccepted { part, accepted } => {
                let here = part
                    .as_deref()
                    .map_or_else(|| "the surface itself".to_owned(), |p| format!("\"{p}\""));
                if accepted.is_empty() {
                    format!("{here} takes no drop")
                } else {
                    format!("this lands on {}, and not on {here}", join_words(accepted))
                }
            }
            Self::NoCommonAction { offered, accepted } => format!(
                "this can {} what is dropped here, and the drag offers only {}",
                join_words(&accepted.wire_names()),
                join_words(&offered.wire_names()),
            ),
            Self::Declined(reason) => reason.as_str().to_owned(),
        }
    }
}

/// `a`, `a and b`, `a, b and c` — the list form the sentences above read as
/// prose. One helper because three arms build a list and a fourth would have
/// been the third copy.
fn join_words(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What a target is shown when a drag is over it.
///
/// Borrowed rather than owned because the router already holds every part of it
/// for the source's own [`DragUpdate`](crate::external::DragUpdate), and a
/// target that wants to keep something takes its own copy of that part.
#[derive(Debug)]
#[non_exhaustive]
pub struct DropOffer<'a> {
    /// What is being dragged.
    pub payload: &'a DragPayload,
    /// Where on this target the cursor is: the tag under it and the cursor
    /// normalised over that tag's rect.
    pub at: &'a DropPoint,
    /// The actions the source and this surface's declaration have in common —
    /// already narrowed by [`DropContract::admits`], so a target chooses from
    /// this set rather than re-deriving the intersection.
    pub actions: DropActions,
    /// The cursor in the window's logical frame, for a target that places
    /// something at the pointer rather than in a cell.
    pub cursor: (f64, f64),
    /// The modifiers held right now, so a target can let a person choose
    /// between the offered actions the way the platforms do.
    pub modifiers: Modifiers,
}

impl<'a> DropOffer<'a> {
    /// An offer over `at`, carrying `payload` and the narrowed `actions`.
    #[must_use]
    pub fn new(
        payload: &'a DragPayload,
        at: &'a DropPoint,
        actions: DropActions,
        cursor: (f64, f64),
        modifiers: Modifiers,
    ) -> Self {
        Self {
            payload,
            at,
            actions,
            cursor,
            modifiers,
        }
    }

    /// The payload's discriminator — what a target matches on first.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.payload.kind
    }

    /// The composite sub-part under the cursor, `None` when the cursor is over
    /// the bare surface tag.
    #[must_use]
    pub fn part(&self) -> Option<&str> {
        crate::composite_tag::split_subindex(&self.at.tag).1
    }
}

/// A target's acceptance, **carrying the landing it previewed**.
///
/// The `landing` is why this type exists rather than a bare `bool`. A target
/// returns one of these on every move (it is the preview) and the router hands
/// the same value back at the release as the commit's witness
/// ([`External::drop_commit`](crate::external::External::drop_commit)) — so a
/// surface cannot commit to a place it did not just show. That is the R1733
/// grid shape lifted to the input contract, and it closes the class the floor
/// leaves open, where the highlight and the outcome are two computations over
/// the same pixel.
///
/// It is an [`IntrospectValue`] rather than a widget-specific type because the
/// router is generic over targets and because it then reaches the wire: an
/// agent asking what a release would do gets the target's own answer
/// (`{"col":2,"row":1}`), not a coordinate it must re-derive.
#[derive(Debug, Clone, PartialEq)]
pub struct DropAccept {
    /// The one action this target will perform, chosen from
    /// [`DropOffer::actions`].
    pub action: DropAction,
    /// What the target would do, in its own words — read by the preview, by
    /// the commit, and by anything asking over the wire.
    pub landing: IntrospectValue,
}

impl DropAccept {
    /// An acceptance that states its landing.
    #[must_use]
    pub const fn new(action: DropAction, landing: IntrospectValue) -> Self {
        Self { action, landing }
    }

    /// An acceptance with nothing structured to say about where it lands.
    ///
    /// Legitimate for a target whose drop has one outcome (a trash well, a
    /// single-slot well), and deliberately still an `IntrospectValue` — the
    /// wire renders `null`, which is an answer, where an absent field would
    /// have been a silence a client cannot tell from an old build.
    #[must_use]
    pub const fn bare(action: DropAction) -> Self {
        Self::new(action, IntrospectValue::Null)
    }
}

/// A target's answer to one [`DropOffer`].
///
/// Two arms, exclusive, and neither is "yes but nothing happens": an
/// acceptance must name its action and its landing, and a refusal must carry a
/// reason. The pair is what makes a claimed-but-inert drop target
/// unrepresentable rather than merely unwanted — the defect R1348 found one
/// layer up, where a zone was claimed and its outcome separately suppressed.
#[derive(Debug, Clone, PartialEq)]
pub enum DropVerdict {
    /// This surface will take it, here, doing this.
    Accept(DropAccept),
    /// It will not, for this reason.
    Refuse(DropRefusal),
}

impl DropVerdict {
    /// Accept, naming the action and the landing.
    #[must_use]
    pub const fn accept(action: DropAction, landing: IntrospectValue) -> Self {
        Self::Accept(DropAccept::new(action, landing))
    }

    /// Refuse for a reason only this surface knows.
    #[must_use]
    pub fn decline(reason: impl Into<RefusalReason>) -> Self {
        Self::Refuse(DropRefusal::declined(reason))
    }

    /// The acceptance, when this is one.
    #[must_use]
    pub const fn accepted(&self) -> Option<&DropAccept> {
        match self {
            Self::Accept(a) => Some(a),
            Self::Refuse(_) => None,
        }
    }

    /// The refusal, when this is one.
    #[must_use]
    pub const fn refused(&self) -> Option<&DropRefusal> {
        match self {
            Self::Refuse(r) => Some(r),
            Self::Accept(_) => None,
        }
    }
}

/// The reserved introspect path a surface's [`DropContract`] is published at,
/// the drop-side peer of the schema path.
///
/// Reserved rather than a declared field because it answers a different
/// question from every other path: not "what is your state" but "what may I
/// hand you". A surface that declares nothing answers an empty array here,
/// which is itself the answer — the floor has no path to ask at all.
pub const DROP_PATH: &str = "$drop";

/// Render a contract as the `$drop` wire value: an array of
/// `{"kind", "actions", "parts"}` objects in declared order.
///
/// Here rather than in the transport because the shape is the *contract's*, and
/// two renderings of one declaration is the drift this workspace has paid for
/// repeatedly. The transport calls this.
#[must_use]
pub fn contract_value(contract: DropContract) -> IntrospectValue {
    let clauses: Vec<serde_json::Value> = contract
        .clauses
        .iter()
        .map(|c| {
            serde_json::json!({
                "kind": c.kind,
                "actions": c.actions.wire_names(),
                "parts": c.named_parts(),
            })
        })
        .collect();
    IntrospectValue::Json(serde_json::Value::Array(clauses))
}

/// Render a refusal as a wire object: its one-word tag, its sentence, and the
/// structured facts each arm carries.
///
/// The tag and the sentence are BOTH rendered because they are read by
/// different clients — an agent matches the tag, a person reads the sentence —
/// and a wire that carries only one of them forces the other reader to parse
/// prose or to render a slug.
///
/// The map is built directly rather than through a `json!` literal that is then
/// re-opened: `as_object_mut` on a literal cannot fail, but it is still an
/// `expect` in a rendering path, and a panic branch that "cannot happen" is one
/// nobody can test. Assembling the map means there is no branch to document.
#[must_use]
pub fn refusal_value(refusal: &DropRefusal) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "refused".into(),
        serde_json::Value::from(refusal.as_wire_name()),
    );
    map.insert("why".into(), serde_json::Value::from(refusal.sentence()));
    match refusal {
        DropRefusal::KindNotAccepted { kind, accepted } => {
            map.insert("kind".into(), serde_json::json!(kind));
            map.insert("accepted".into(), serde_json::json!(accepted));
        }
        DropRefusal::PartNotAccepted { part, accepted } => {
            map.insert("part".into(), serde_json::json!(part));
            map.insert("accepted".into(), serde_json::json!(accepted));
        }
        DropRefusal::NoCommonAction { offered, accepted } => {
            map.insert("offered".into(), serde_json::json!(offered.wire_names()));
            map.insert("accepted".into(), serde_json::json!(accepted.wire_names()));
        }
        DropRefusal::Declined(_) => {}
    }
    serde_json::Value::Object(map)
}

/// The [`DragPayload::kind`] the analysis board's palette hands the board.
///
/// In core because both ends name it — the palette that produces it and the
/// board that declares it accepted — and a kind spelled twice is a kind that
/// stops matching once. Same reasoning as the dock's panel-drag kind.
pub const BOARD_WIDGET_DRAG_KIND: &str = "board-widget";

#[cfg(test)]
mod tests {
    use super::{
        BOARD_WIDGET_DRAG_KIND, DROP_PATH, DropAccept, DropAction, DropActions, DropClause,
        DropContract, DropOffer, DropRefusal, DropRegion, DropVerdict, contract_value,
        refusal_value,
    };
    use crate::external::{DragPayload, DropPoint, IntrospectValue};
    use crate::input::Modifiers;

    const COPY_MOVE: DropActions = DropActions::one(DropAction::Copy).with(DropAction::Move);

    const BOARD: DropContract = DropContract::new(
        const {
            &[
                DropClause::surface(BOARD_WIDGET_DRAG_KIND, COPY_MOVE),
                DropClause::parts(
                    "saved-layout",
                    DropActions::one(DropAction::Link),
                    const { &["header", "footer"] },
                ),
            ]
        },
    );

    fn point(tag: &str) -> DropPoint {
        DropPoint {
            tag: tag.to_owned(),
            x_rel: 0.5,
            y_rel: 0.5,
        }
    }

    #[test]
    fn r1734_an_action_set_is_never_empty() {
        // The type has no empty value and `intersect` refuses to invent one, so
        // "accepts the kind, can do nothing with it" cannot be built.
        assert_eq!(DropActions::one(DropAction::Copy).count(), 1);
        assert_eq!(COPY_MOVE.count(), 2);
        assert_eq!(
            DropActions::one(DropAction::Copy).intersect(DropActions::one(DropAction::Move)),
            None,
        );
        assert_eq!(COPY_MOVE.first(), DropAction::Copy);
        assert_eq!(DropActions::one(DropAction::Link).first(), DropAction::Link);
    }

    #[test]
    fn r1734_every_action_round_trips_through_its_wire_name() {
        for action in DropAction::ALL {
            assert_eq!(
                DropAction::from_wire_name(action.as_wire_name()),
                Some(action)
            );
        }
        assert_eq!(DropAction::from_wire_name("teleport"), None);
        assert_eq!(COPY_MOVE.wire_names(), ["copy", "move"]);
    }

    #[test]
    fn r1734_an_undeclared_kind_is_refused_with_the_declared_ones() {
        let refusal = BOARD
            .admits("packet-row", COPY_MOVE, None)
            .expect_err("the board declares no packet-row clause");
        match &refusal {
            DropRefusal::KindNotAccepted { kind, accepted } => {
                assert_eq!(kind, "packet-row");
                assert_eq!(accepted, &[BOARD_WIDGET_DRAG_KIND, "saved-layout"]);
            }
            other => panic!("expected a kind refusal, got {}", other.as_wire_name()),
        }
        // The sentence names what WOULD have worked — the whole difference from
        // the floor's bare boolean.
        assert!(refusal.sentence().contains(BOARD_WIDGET_DRAG_KIND));
        assert!(refusal.sentence().contains("saved-layout"));
    }

    #[test]
    fn r1734_a_declared_kind_on_the_wrong_part_names_the_right_parts() {
        let refusal = BOARD
            .admits(
                "saved-layout",
                DropActions::one(DropAction::Link),
                Some("cell-3"),
            )
            .expect_err("saved-layout lands on header or footer only");
        match &refusal {
            DropRefusal::PartNotAccepted { part, accepted } => {
                assert_eq!(part.as_deref(), Some("cell-3"));
                assert_eq!(accepted, &["header", "footer"]);
            }
            other => panic!("expected a part refusal, got {}", other.as_wire_name()),
        }
        assert!(refusal.sentence().contains("header"));
        // And the bare surface is NOT one of the named parts.
        assert!(
            BOARD
                .admits("saved-layout", DropActions::one(DropAction::Link), None)
                .is_err()
        );
        assert!(
            BOARD
                .admits(
                    "saved-layout",
                    DropActions::one(DropAction::Link),
                    Some("header")
                )
                .is_ok()
        );
    }

    #[test]
    fn r1734_no_common_action_is_a_refusal_that_names_both_sides() {
        let refusal = BOARD
            .admits(
                BOARD_WIDGET_DRAG_KIND,
                DropActions::one(DropAction::Link),
                None,
            )
            .expect_err("the board cannot link a widget");
        match &refusal {
            DropRefusal::NoCommonAction { offered, accepted } => {
                assert_eq!(offered.wire_names(), ["link"]);
                assert_eq!(accepted.wire_names(), ["copy", "move"]);
            }
            other => panic!("expected an action refusal, got {}", other.as_wire_name()),
        }
        let why = refusal.sentence();
        assert!(why.contains("copy and move"), "{why}");
        assert!(why.contains("link"), "{why}");
    }

    #[test]
    fn r1734_admitting_narrows_to_the_common_actions() {
        let common = BOARD
            .admits(
                BOARD_WIDGET_DRAG_KIND,
                DropActions::one(DropAction::Move),
                None,
            )
            .expect("move is one of the board's two");
        assert_eq!(common.wire_names(), ["move"]);
        let both = BOARD
            .admits(BOARD_WIDGET_DRAG_KIND, COPY_MOVE, Some("cell-3"))
            .expect("a surface clause covers every part");
        assert_eq!(both.wire_names(), ["copy", "move"]);
    }

    #[test]
    fn r1734_an_empty_contract_admits_nothing_and_says_so() {
        let refusal = DropContract::EMPTY
            .admits(BOARD_WIDGET_DRAG_KIND, COPY_MOVE, None)
            .expect_err("an empty contract is not a drop target");
        assert_eq!(refusal.as_wire_name(), "kind-not-accepted");
        assert_eq!(
            refusal.sentence(),
            "nothing can be dropped here, and this is a board-widget"
        );
        assert!(DropContract::EMPTY.is_empty());
        assert!(!BOARD.is_empty());
    }

    #[test]
    fn r1734_a_verdict_is_accept_or_refuse_and_never_both() {
        let ok = DropVerdict::accept(DropAction::Copy, IntrospectValue::Int(7));
        assert_eq!(ok.accepted().map(|a| a.action), Some(DropAction::Copy));
        assert!(ok.refused().is_none());
        let no = DropVerdict::decline("the board is full");
        assert!(no.accepted().is_none());
        assert_eq!(
            no.refused().map(DropRefusal::as_wire_name),
            Some("declined")
        );
        assert_eq!(
            no.refused().map(DropRefusal::sentence).unwrap(),
            "the board is full"
        );
        // A bare acceptance still SAYS something on the wire.
        assert_eq!(
            DropAccept::bare(DropAction::Move).landing,
            IntrospectValue::Null
        );
    }

    #[test]
    fn r1734_an_offer_reads_its_part_off_the_composite_tag() {
        let payload = DragPayload::new(BOARD_WIDGET_DRAG_KIND, IntrospectValue::Text("t".into()));
        let composite = point("board#cell-2-1");
        let offer = DropOffer::new(
            &payload,
            &composite,
            COPY_MOVE,
            (10.0, 20.0),
            Modifiers::empty(),
        );
        assert_eq!(offer.kind(), BOARD_WIDGET_DRAG_KIND);
        assert_eq!(offer.part(), Some("cell-2-1"));
        let bare = point("board");
        let offer = DropOffer::new(&payload, &bare, COPY_MOVE, (10.0, 20.0), Modifiers::empty());
        assert_eq!(offer.part(), None);
    }

    #[test]
    fn r1734_the_contract_renders_the_declaration_and_nothing_else() {
        let IntrospectValue::Json(value) = contract_value(BOARD) else {
            panic!("$drop renders an array");
        };
        assert_eq!(
            value,
            serde_json::json!([
                {"kind": "board-widget", "actions": ["copy", "move"], "parts": []},
                {"kind": "saved-layout", "actions": ["link"], "parts": ["header", "footer"]},
            ]),
        );
        let IntrospectValue::Json(empty) = contract_value(DropContract::EMPTY) else {
            panic!("$drop renders an array");
        };
        assert_eq!(empty, serde_json::json!([]));
        assert_eq!(DROP_PATH, "$drop");
    }

    #[test]
    fn r1734_a_rendered_refusal_carries_both_readers_answers() {
        let refusal = BOARD.admits("packet-row", COPY_MOVE, None).unwrap_err();
        let value = refusal_value(&refusal);
        assert_eq!(value["refused"], "kind-not-accepted");
        assert_eq!(value["kind"], "packet-row");
        assert_eq!(
            value["accepted"],
            serde_json::json!(["board-widget", "saved-layout"])
        );
        assert!(value["why"].as_str().unwrap().contains("board-widget"));
        // The sentence is prose for a person; the tag is a token for an agent.
        // Neither is derivable from the other, which is why both are rendered.
        assert_ne!(value["refused"], value["why"]);
    }

    #[test]
    fn r1734_a_part_clause_declares_which_parts_and_a_surface_clause_declares_none() {
        let surface = DropClause::surface("k", COPY_MOVE);
        assert_eq!(surface.region, DropRegion::Surface);
        assert_eq!(surface.named_parts(), &[] as &[&str]);
        assert!(surface.admits_part(None));
        assert!(surface.admits_part(Some("anything")));
        let parts = DropClause::parts("k", COPY_MOVE, const { &["a"] });
        assert_eq!(parts.named_parts(), &["a"]);
        assert!(!parts.admits_part(None));
        assert!(parts.admits_part(Some("a")));
        assert!(!parts.admits_part(Some("b")));
    }
}
