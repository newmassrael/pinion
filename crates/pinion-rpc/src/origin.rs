//! R1487 §5.12 §2 #7 — the surface vocabulary shared by every method that
//! can be answered by more than one scene.
//!
//! R1482 introduced [`AnswerOrigin`] for `scene/query`'s answers and R1485
//! extended it to `scene/query`'s refusals, both living in [`crate::query`](mod@crate::query).
//! R1487 gave the write channel the same disclosure, at which point the
//! vocabulary had four consumers across three modules and its home in the
//! read module was an accident of which round needed it first
//! ([[helper-crate-home-ssot-axis]]).
//!
//! Nothing here decides *policy*: which surface a walk reached is a fact each
//! method's own resolution establishes. This module owns only the words for
//! it, and [`Refusal`], the one shape a refusal-plus-surface takes — so a
//! third method cannot invent a fourth spelling of the same pair.

use pinion_core::utterance::Announced;

use crate::query::QueryError;
use crate::resolve::ResolveExternalError;

/// R1482 §5.12 — which of the two scenes a caller handed a `*_from` entry
/// point.
///
/// The same two words `scene/snapshot` already puts on the wire for its
/// `from` param, because they name the same duality: the binding's
/// retained state scene, and the last painted frame. Naming it as a type
/// (rather than passing a `bool`) is what lets [`AnswerOrigin::of`] be
/// the single place that decides what a hit in each scene means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneSource {
    /// The application's retained state scene.
    State,
    /// The last painted frame.
    Paint,
}

/// R1482 §5.12 §2 #7 — what an outcome *is*, beyond its value: the surface
/// that produced it.
///
/// A value alone cannot be checked for freshness, and the three ways an
/// outcome can arrive have three different contracts while producing
/// byte-identical wire results:
///
/// * [`State`](Self::State) — the retained state scene answered. That
///   scene is not rebuilt per frame, so the value is the model's current
///   one whatever node kind held it, and a write into it persists.
/// * [`PaintDriver`](Self::PaintDriver) — an
///   [`ImmediateModeNode`](pinion_core::scene::ImmediateModeNode) in the
///   painted frame answered. Its `handle` is the same `Rc` the tick loop
///   drives, so the value is current simulation state and a write reaches
///   the object the loop is advancing — this is the case R828 built the
///   read fallback *for*, and R1481 the write fallback.
/// * [`PaintFrame`](Self::PaintFrame) — a retained node in the painted
///   frame answered. The view fn rebuilds that node every paint, so the
///   value is the one it carried **as of the last painted frame**, and a
///   write into it would be discarded before the next one.
///
/// `PaintFrame` is a statement of provenance, not a verdict of staleness:
/// for an external whose value does not outlive the frame it was built
/// from, as-of-last-paint *is* the right answer, which is why the read
/// fallback stays open to it where the R1481 write fallback does not.
/// What the caller could not do before was tell the three apart.
///
/// The name records where the vocabulary was born (R1482, on the answer
/// channel). It has named refusals since R1485 and write outcomes since
/// R1487; one imperfect name for one concept is the cheaper mistake than
/// two names for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerOrigin {
    /// The retained state scene answered.
    State,
    /// A live immediate-mode driver in the painted frame answered.
    PaintDriver,
    /// A per-frame node in the painted frame answered.
    PaintFrame,
}

impl AnswerOrigin {
    /// The single site that decides what a hit means, so the scene the
    /// dispatcher chose and the node kind the walker found cannot be
    /// combined two different ways in two places.
    ///
    /// `driver` says the walk landed on an immediate-mode node. It is
    /// deliberately ignored for [`SceneSource::State`]: the state scene
    /// persists across frames, so neither node kind makes its answer
    /// as-of-a-frame, and reporting a `StateDriver` would invite a caller
    /// to treat a distinction that carries no freshness meaning as if it
    /// did.
    #[must_use]
    pub fn of(source: SceneSource, driver: bool) -> Self {
        match (source, driver) {
            (SceneSource::State, _) => Self::State,
            (SceneSource::Paint, true) => Self::PaintDriver,
            (SceneSource::Paint, false) => Self::PaintFrame,
        }
    }

    /// The wire word. One mapping, so the schema and the answer cannot
    /// drift apart.
    #[must_use]
    pub fn to_wire(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::PaintDriver => "paint_driver",
            Self::PaintFrame => "paint_frame",
        }
    }
}

/// R1485 §5.12 §2 #7 — a refusal, and the surface that produced it.
///
/// [`AnswerOrigin`] gave an *answer* its provenance and left a refusal with
/// none, because the answer and the refusal did not travel the same type: a
/// `*_from` entry point returned the origin beside the value, so its error
/// arm had nowhere to carry one. That was a shape, not a decision, and it
/// cost a caller a fact it cannot derive — every one of these methods
/// retries the painted frame when the state scene has nothing at the path
/// (R828 read, R1481 write), so a bare reason word may have come from either
/// scene, and from either kind of node within the painted one.
///
/// `refused_by` is `Some` exactly when the walk **identified a surface at
/// the address**, whether that surface then declined (it does not declare
/// the path, or exposes nothing) or the frame contract declined on its
/// behalf (a per-frame node cannot take a write). It is `None` for the
/// refusals that name no node at all — a malformed path, an unsupported
/// shape, or nothing at the address. The absence is itself the statement:
/// nothing was reached, so nothing can be named.
///
/// The rule is *the read's own resolution*, so the two channels cannot
/// disagree about what exists: an address where a query identifies a
/// surface is an address where a write names one too, even when the write
/// is refused ([[wire-form-read-write-symmetry]]).
///
/// The word is checkable against something independent of itself. A
/// refusal's origin names the same surface that surface's *successful*
/// reads name, so it says where the matching `$schema` lives — which turns
/// "no" into a next step rather than a dead end.
///
/// R1487 — generic over the error type because `scene/query`,
/// `scene/invoke` and `scene/intervene` all needed exactly this pair. The
/// third consumer is what lifted it; two would have been a guess
/// ([[abstraction-needs-second-consumer]]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal<E> {
    /// Why the call failed — the same reason the origin-less projection of
    /// the same dispatcher reports.
    pub error: E,
    /// The surface that declined, when one was identified.
    pub refused_by: Option<AnswerOrigin>,
    /// ★★★★★ R1720 — **whether the person in front of that surface was told**,
    /// for the two channels that ask.
    ///
    /// The third fact about a refusal, and the three arrived in the order a
    /// caller needed them: R1487 said *which surface* refused, R1564 said *what
    /// it refused about*, and this says *whether anyone at the screen heard*.
    ///
    /// It is here because an agent's next move depends on it. An agent that
    /// knows the person was already told does not have to say it again, and one
    /// that knows they were not can decide to. Before this the agent could not
    /// tell those apart, so the choice was made per screen and made
    /// differently: measured, one of the three announced a refusal and two were
    /// silent.
    ///
    /// `None` for the read channel and for a refusal decided before any surface
    /// was reached. Nothing was asked, so nothing is claimed — the same
    /// direction `refused_by` takes for the address that named no node.
    pub announced: Option<Announced>,
}

impl<E> Refusal<E> {
    /// A refusal decided before any surface was identified.
    pub(crate) fn unreached(error: impl Into<E>) -> Self {
        Self {
            error: error.into(),
            refused_by: None,
            announced: None,
        }
    }

    /// A refusal an identified surface produced, named by `origin`.
    pub(crate) fn from_surface(error: E, origin: AnswerOrigin) -> Self {
        Self {
            error,
            refused_by: Some(origin),
            announced: None,
        }
    }

    /// R1720 — the same refusal, recording what the surface did about telling
    /// the person.
    ///
    /// Separate from [`from_surface`](Self::from_surface) because the two facts
    /// are established at different moments: the origin is bound before the
    /// branches (R1487), and whether the person was told is only known after
    /// the surface has been asked.
    pub(crate) fn announcing(mut self, announced: Announced) -> Self {
        self.announced = Some(announced);
        self
    }
}

impl<E: From<ResolveExternalError>> Refusal<E> {
    /// R1487 — classify a [`crate::resolve`](mod@crate::resolve) failure into "a surface said
    /// no" and "the walk found no surface".
    ///
    /// [`IntrospectionOptedOut`](ResolveExternalError::IntrospectionOptedOut)
    /// is the one resolve failure where the node IS there and declined to
    /// expose a channel. That is a fact about one node in one scene, so it
    /// names it: the caller learns the address was right and the door is
    /// shut, which is not what "there is nothing here" says. Every other
    /// resolve failure ended the walk without identifying a surface —
    /// including the `primary_external` miss, where a node was found but
    /// exposes no External head.
    ///
    /// The per-method error enums each already map
    /// `ResolveExternalError` onto their own variants, so this needs no
    /// variant argument: one classification, three dispatchers
    /// (`scene/query`, `scene/invoke`, `scene/intervene`), which is what
    /// lifted it out of the third hand-written copy
    /// ([[three-site-internal-duplication-substrate-lift]]).
    pub(crate) fn from_resolve(err: ResolveExternalError, origin: AnswerOrigin) -> Self {
        match err {
            ResolveExternalError::IntrospectionOptedOut => Self::from_surface(err.into(), origin),
            other => Self::unreached(other),
        }
    }
}

/// The refusal shape [`crate::query_from`] reports.
pub type QueryRefusal = Refusal<QueryError>;
