//! `scene/invoke` RPC method dispatch — R17 bidirectional RPC spec
//! round, eighth typed handler extending the §5.12 set from 7 to 8.
//!
//! Wires the same three pieces as [`crate::query`](fn@crate::query) / [`crate::rewind`](fn@crate::rewind):
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>`
//!      with single-window short-circuit.
//!   2. **§5.2 scene tree walk** — R666 §5.34 v1 multi-External
//!      addressing: `path::split_at_external` returns scene segments +
//!      introspect path; [`Scene::lookup_path_mut`] walks Container /
//!      Scroll by tag/index until it reaches the addressed
//!      [`ExternalNode`](pinion_core::scene::ExternalNode); [`Scene::primary_external_mut`] then descends
//!      the R55.D.5 multi-widget container shape to the substrate's
//!      first External.
//!   3. **§5.15 item 8 invoke dispatch** — descend through
//!      [`External::introspect_mut`](pinion_core::external::External::introspect_mut) and consult the
//!      [`ExternalIntrospect::invoke`] action channel.
//!
//! v1 scene-path syntax accepted:
//!
//!   * `/external/<action>` — primary External at root (v0 retained).
//!   * `/<tag>/external/<action>` — DFS lookup by `ExternalNode` tag.
//!     Composite tags (`todo_toggle#1`) pass through verbatim, so
//!     R55.D.5 `create_extra_externals` siblings are addressable
//!     without per-binding RPC plumbing.
//!   * `/<seg>/.../<tag>/external/<action>` — nested Container walk
//!     followed by tagged child match (mirror of [`crate::query`](fn@crate::query)).
//!
//! Other shapes return [`InvokeError::UnsupportedPath`].
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is handled by
//! [`crate::dispatch`](fn@crate::dispatch); this module exposes the typed dispatcher
//! only.

use std::borrow::Cow;

use pinion_core::Scene;
use pinion_core::external::{
    ExternalIntrospect, IntrospectValue, InvokeError as TraitInvokeError, RefusalReason,
    SchemaChannel,
};
use pinion_core::utterance::{Announced, Tone, Utterance};

use crate::origin::{AnswerOrigin, Refusal, SceneSource};
use crate::path::PathError;
use crate::resolve::{
    ResolveExternalError, external_node_at, lookup_addressed, resolve_external_introspect_mut,
    resolve_external_path,
};

/// Reasons the typed [`invoke`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Path expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// The `External` did not opt in to §5.15 item 8 introspection
    /// (so the invoke channel is unreachable).
    IntrospectionOptedOut,
    /// The action path is not declared in the `External`'s schema **on any
    /// channel**, so the call was refused without reaching the surface
    /// (R1637 — see the crate-private `invoke_declared`).
    ///
    /// R1566 also answered this for a name the schema declared as an action
    /// and the surface's own dispatch then did not recognise. That is a
    /// different fact about a different surface, and it now has its own word
    /// ([`DeclaredButUnhandled`](Self::DeclaredButUnhandled)).
    UnknownInvokePath,
    /// R1566 §2 #7 — the path is declared, on the **read** channel: it is a
    /// slot to `query` and `intervene`, not an action to call.
    ///
    /// The toolkit fuses this too. `invokeMethod()` on a property name
    /// answers `false`, and the caller is left to discover from the
    /// meta-object that what they named was a meta-property all along.
    PathIsAReadSlot,
    /// The args variant does not match the action's declared type.
    InvokeTypeMismatch,
    /// The action declined to fire, **stating why** — R1564 §5.15 §2 #2
    /// (PINION-PR82).
    ///
    /// The payload is the producer's own [`RefusalReason`], forwarded verbatim
    /// to the JSON-RPC `error.data`. Before R1564 this variant carried nothing
    /// and the wire published the string `"InvokeRejected"`, which names the
    /// transport's classification and not the fact the surface observed — so a
    /// consumer had no material to build a message out of, and the measured
    /// result downstream was six of sprag's fifteen CLI failure paths printing
    /// an `or`-joined guess at causes their own daemon had already told apart.
    ///
    /// Unlike every other variant here, the sentence is **not** this crate's to
    /// author: the transport forwards it. That is what makes the refusal
    /// attributable in the same sense R1487's [`AnswerOrigin`] made it — R1487
    /// said which surface refused, and this says what it refused about.
    InvokeRejected(RefusalReason),
    /// R1564 — the surface reported a [`TraitInvokeError`] this transport has
    /// not been taught to name.
    ///
    /// [`TraitInvokeError`] is `#[non_exhaustive]`, so the conversion below
    /// needs a wildcard arm. Pre-R1564 that arm answered `InvokeRejected`,
    /// which was a *guess* nothing paid for while the variant was empty and
    /// becomes a fabrication once it has to carry a producer's sentence: the
    /// transport would be inventing a reason for a refusal it does not
    /// understand, and an operator would read an authored-looking statement
    /// with no author. Naming the situation is the honest answer, and it is the
    /// same correction R1487 made to `NoExternalAtPath`.
    ///
    /// Unreachable today by construction — no third failure mode exists — so it
    /// is a compile-time-visible landing site rather than a tested path, which
    /// is exactly its purpose: the round that adds a variant upstream finds
    /// this arm instead of shipping through it.
    UnmappedSurfaceError,
    /// R1637 §2 #2 §2 #7 — the schema declared this name on the **invoke**
    /// channel and the surface's own dispatch then did not recognise it.
    ///
    /// A statement about the *surface*, not about the caller: the caller read
    /// `$schema`, asked for exactly what it published, and the publisher did
    /// not answer. Before R1637 this shared [`Self::UnknownInvokePath`] with "you
    /// made that name up", so an agent doing the right thing and an agent
    /// guessing were told the same word — and only one of them had found a
    /// bug.
    ///
    /// **The reference cannot represent this case, and cannot report it
    /// either.** The toolkit's meta-object is generated from the definitions, so a
    /// declared
    /// method is dispatchable by construction; the mirror-image inconsistency —
    /// hand-writing a declaration beside a hand-written dispatch — is a shape
    /// it does not have. pinion's surfaces do hand-write both, so the
    /// disagreement is real here, and naming it is what lets a client
    /// distinguish "this surface is inconsistent" from "I asked wrong".
    DeclaredButUnhandled,
    /// R1487 §2 #7 — the address named a **retained** `External` reached
    /// through the shared ([`invoke_shared`]) walk, which cannot act on it.
    ///
    /// R1481 already refused this case and refused it correctly: in a
    /// painted frame a retained node is a `Box` the view fn rebuilds every
    /// paint, so an action taken on it would be discarded before the next
    /// one. What it reported was `NoExternalAtPath` — "there is no external
    /// at that path" — about an address `scene/query` resolves and names.
    /// The refusal survives; the false statement about the scene does not
    /// ([[wire-form-read-write-symmetry]]).
    RetainedNodeNotWritable,
}

/// A refused [`invoke_from`], and the surface that refused it.
pub type InvokeRefusal = Refusal<InvokeError>;

impl From<PathError> for InvokeError {
    fn from(err: PathError) -> Self {
        InvokeError::Path(err)
    }
}

impl From<ResolveExternalError> for InvokeError {
    fn from(err: ResolveExternalError) -> Self {
        match err {
            ResolveExternalError::Path(e) => InvokeError::Path(e),
            ResolveExternalError::UnsupportedPath => InvokeError::UnsupportedPath,
            ResolveExternalError::NoExternalAtPath => InvokeError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => InvokeError::IntrospectionOptedOut,
        }
    }
}

impl InvokeError {
    /// ★★★★★ R1720 §5.15 §2 #2 — **what the person in front of the surface is
    /// told when this refusal happens.**
    ///
    /// The wire's own word for a refusal is a TAG an agent matches on —
    /// `"UnknownInvokePath"`, `"PathIsAReadSlot"`, rendered by this crate's
    /// dispatch module. This is a sentence a person reads. Both come off this
    /// one value, so they cannot drift, which is the same split [`Utterance`]
    /// made between a clause and the frame put in front of it.
    ///
    /// Every arm speaks, and no two read alike: the arms exist because a caller
    /// can act differently on each, and a person watching the screen has the
    /// same claim on the difference. `r1720_every_invoke_refusal_is_said_and_distinct`
    /// is what holds that.
    ///
    /// # The one arm this crate does not author
    ///
    /// [`InvokeRejected`](Self::InvokeRejected) carries the producer's own
    /// sentence, forwarded verbatim — R1564's decision, kept. When that
    /// sentence cannot be said as it stands — it is empty, it already carries
    /// the frame, or it is a `Debug` spelling, which are the three faults
    /// `pinion_core::utterance` names — this answers a sentence of its own
    /// instead. It does **not** panic: unlike a screen composing its own
    /// announcement, the clause here has an agent's argument interpolated into
    /// it, so a panic would be a way to stop the process from the wire.
    #[must_use]
    pub fn said(&self) -> Utterance {
        let clause: Cow<'static, str> = match self {
            // Addressing. The call never reached a surface, so these are the
            // framework speaking about the address the agent named.
            Self::Path(_) => "that address is not one this window can read".into(),
            Self::UnsupportedPath => "that address is not a shape this window resolves".into(),
            Self::NoExternalAtPath => "there is nothing at that address to act on".into(),
            Self::IntrospectionOptedOut => "the surface at that address takes no calls".into(),
            Self::RetainedNodeNotWritable => {
                "that address names a drawn node, which cannot be acted on".into()
            }
            // The channel question, settled before dispatch (R1637).
            Self::UnknownInvokePath => "there is no such action on this surface".into(),
            Self::PathIsAReadSlot => "that is something to read, not something to do".into(),
            // What the surface itself reported.
            Self::InvokeTypeMismatch => "that argument is not the kind this action takes".into(),
            Self::DeclaredButUnhandled => {
                "this surface publishes that action and did not answer it".into()
            }
            Self::UnmappedSurfaceError => {
                "this surface refused in a way the wire cannot name".into()
            }
            Self::InvokeRejected(reason) => {
                return Utterance::checked(Tone::Refused, reason.as_str()).unwrap_or_else(|_| {
                    // R1720 — true of all three faults, and it quotes nothing:
                    // the surface's words are what could not be shown, so
                    // repeating them here would be showing them.
                    Utterance::refused(&"this surface refused, and its reason cannot be shown")
                });
            }
        };
        Utterance::new(Tone::Refused, clause)
    }

    /// R1566 §2 #7 §5.12 — the wire refusal for a surface that declined a call
    /// its **own declaration had already admitted** (R1637 moved that judgement
    /// ahead of dispatch — see `invoke_declared`, which is this function's
    /// only caller).
    ///
    /// It takes neither the schema nor the path any more, and that is the
    /// point: by the time a surface can decline, the channel question is
    /// settled. `UnknownPath` from here therefore cannot mean "no such name" —
    /// the name was published — so it lands on
    /// [`DeclaredButUnhandled`](InvokeError::DeclaredButUnhandled) rather than
    /// sharing a word with the caller's own mistake.
    #[must_use]
    fn from_dispatched(err: TraitInvokeError) -> Self {
        // `TraitInvokeError` is `#[non_exhaustive]`, so the wildcard is
        // mandatory here. R1564 — it no longer answers `InvokeRejected`: see
        // [`UnmappedSurfaceError`](InvokeError::UnmappedSurfaceError) for why a
        // reason-carrying variant must not be the wildcard's landing site.
        match err {
            TraitInvokeError::UnknownPath => InvokeError::DeclaredButUnhandled,
            TraitInvokeError::TypeMismatch => InvokeError::InvokeTypeMismatch,
            TraitInvokeError::Rejected(reason) => InvokeError::InvokeRejected(reason),
            _ => InvokeError::UnmappedSurfaceError,
        }
    }
}

/// R1637 §2 #2 §5.12 §5.15 — dispatch an action **only if the surface declared
/// it**, and classify what comes back.
///
/// The one place either branch of this module reaches a surface's `invoke`, for
/// the reason R828 gave `introspect_or_schema`: two call sites that each decide
/// what is callable are two definitions of the wire contract, and they drift.
///
/// # Why the declaration is a precondition and not a diagnosis
///
/// Until R1637 the declaration was consulted **after** the surface declined, so
/// it could only ever explain a refusal. That left the dangerous direction —
/// implemented but never declared — with no channel able to observe it at all:
/// the action worked, and nothing in the workspace could tell it apart from one
/// that had been published. It is not hypothetical. R1631 added an `arrange`
/// verb to `hello-node-groups` and did not add it to the schema; for a full
/// round it was callable and undiscoverable, and `cargo test`, clippy, the demo
/// and the push gate all passed, because none of them had anything to compare.
///
/// Ordering the check first turns that into an impossibility rather than a
/// warning. §2 #2 makes RPC the AI's primary path, so the surface an agent can
/// *find* has to be the surface it can *reach*; a name absent from `$schema` is
/// now absent from the wire, and the round that forgets to declare one finds
/// out from its own demo.
///
/// **This is the reference's floor, not a pinion invention.** The toolkit's
/// meta-object is the only route to an object's action channel, and it is
/// generated from the
/// declarations, so an implemented-but-undeclared method is unreachable by
/// construction. Measured on the toolkit at 6.4.2 with a class carrying one
/// `slots:` method
/// and one plain one: `invokeMethod` answered `true` for the declared name and
/// ran it, answered `false` for the undeclared one and did **not** run it
/// (`indexOfMethod` = -1), and its 6.11.1 source still emits the same
/// `"No such method"` warning from the meta-object's own translation unit.
/// pinion reaches the same
/// guarantee by gate rather than by codegen, because its surfaces are
/// hand-written — and gets one report out of it the meta-object cannot express,
/// [`DeclaredButUnhandled`](InvokeError::DeclaredButUnhandled).
///
/// # What it costs
///
/// One linear scan of a `&'static [SchemaField]` per call — the schema is
/// `Copy` over a static slice, so nothing is allocated or cloned. R1566's note
/// that "a call that fired must not pay for a judgement it does not need" no
/// longer applies: the judgement IS the contract, and it is paid at RPC
/// cadence, not per frame.
///
/// # What it does not cover
///
/// In-process dispatch. A binding that calls `ExternalIntrospect::invoke`
/// directly — a keybinding forwarding a verb, say — does not pass through here,
/// and the framework has no seam that could intercept it. The contract this
/// enforces is the **wire** contract: what is callable over RPC is exactly what
/// `$schema` publishes.
fn invoke_declared(
    intro: &mut dyn ExternalIntrospect,
    action_path: &str,
    args: IntrospectValue,
) -> Result<IntrospectValue, (InvokeError, Announced)> {
    let outcome = match intro.schema().field_for(action_path).map(|f| f.channel) {
        Some(SchemaChannel::Invoke) => intro
            .invoke(action_path, args)
            .map_err(InvokeError::from_dispatched),
        Some(SchemaChannel::Read) => Err(InvokeError::PathIsAReadSlot),
        // `None` is the undeclared name. The wildcard is `SchemaChannel` being
        // `#[non_exhaustive]`: a channel this transport has not been taught is
        // one it cannot claim admits a call, so it refuses rather than guessing
        // — the same direction `UnmappedSurfaceError` chose for its own
        // unknown.
        _ => Err(InvokeError::UnknownInvokePath),
    };
    // ★★★★★ R1720 §5.15 §2 #2 — **both channels, from one value, in the one
    // place either branch reaches a surface.** The refusal handed back to the
    // agent and the sentence put in front of the person are the same
    // `Utterance`; there is no site where a producer can supply one and forget
    // the other, because there is no site at all.
    //
    // Measured before this line existed: 55 of the three screens' action slots
    // refuse and **2** reached the person, those two being the two places
    // somebody had written the pair out by hand.
    outcome.map_err(|err| {
        let announced = intro.announce(&err.said());
        (err, announced)
    })
}

/// Resolve `raw_path` against `scene` and invoke the action there
/// with `args`, returning the action's typed result value.
///
/// See module docs for the v0 path syntax. The `scene` reference is
/// borrowed mutably for the lifetime of the call so the underlying
/// `External` can advance its state.
///
/// # Errors
///
/// Returns [`InvokeError`] when the path is malformed, the scene root
/// does not match the path shape, or the underlying `External`
/// rejects the action.
pub fn invoke(
    scene: &mut Scene,
    raw_path: &str,
    args: IntrospectValue,
) -> Result<IntrospectValue, InvokeError> {
    invoke_from(scene, SceneSource::State, raw_path, args)
        .map(|(value, _)| value)
        .map_err(|refusal| refusal.error)
}

/// Resolve `raw_path` against `scene`, invoke the action there with `args`,
/// and report [which surface acted](AnswerOrigin).
///
/// `source` names the scene the caller is handing over; the walk itself is
/// identical either way. [`invoke`] is the projection that drops the origin,
/// for the callers that hold only one scene and so already know it — the
/// same pairing [`crate::query`](fn@crate::query) / [`crate::query_from`]
/// have had since R1482.
///
/// # Errors
///
/// Returns an [`InvokeRefusal`] carrying the same [`InvokeError`] [`invoke`]
/// reports, plus the surface that produced it when the walk identified one.
pub fn invoke_from(
    scene: &mut Scene,
    source: SceneSource,
    raw_path: &str,
    args: IntrospectValue,
) -> Result<(IntrospectValue, AnswerOrigin), InvokeRefusal> {
    // R667 §5.34 — `/window[id]/<segs>/external/<action>` parse +
    // Container/Scroll walk + multi-widget primary descent + §5.15
    // introspect-mut lookup lifted into [`resolve_external_introspect_mut`].
    // `action_path` is owned (String) so the `&mut dyn ExternalIntrospect`
    // borrow on `scene` can outlive `raw_path`'s lifetime.
    if let Some(result) = invoke_immediate_at(scene, source, raw_path, &args) {
        return result;
    }
    // R1487 — the origin is bound once, before the branches, so an action
    // and a refusal from this same node cannot report two different
    // surfaces. Each stage then attaches it where that stage *knows* what
    // it reached, rather than a later classifier re-deriving it from the
    // error variant: the same variant arrives with a surface identified and
    // without one.
    let origin = AnswerOrigin::of(source, false);
    let (intro, action_path) = resolve_external_introspect_mut(scene, raw_path)
        .map_err(|e| InvokeRefusal::from_resolve(e, origin))?;
    invoke_declared(intro, &action_path, args)
        .map(|value| (value, origin))
        .map_err(|(err, announced)| InvokeRefusal::from_surface(err, origin).announcing(announced))
}

/// R1481 §2 #4 §5.12 — the immediate-mode half of [`invoke`], reachable
/// through a **shared** `&Scene`.
///
/// R828 gave `scene/query` an immediate-mode branch and deferred this one
/// with a stated reason: `invoke` handed back a `&mut dyn ExternalIntrospect`
/// that had to outlive resolution, which a `RefCell` borrow cannot do. The
/// deferral was conditioned on "until a consumer needs it" — and the consumer
/// arrived as a defect: `scene/query ball/external/velocity` answered `-2.5`
/// while `scene/invoke` on the same path answered `NoExternalAtPath`, a
/// refusal the read had just disproved.
///
/// The seam dissolves the same way it did for the read: do the work *inside*
/// the borrow and return an owned value, instead of returning the borrow.
/// `Rc<RefCell<dyn ImmediateMode>>` is interior-mutable, so a shared `&Scene`
/// is enough — which is what lets the last painted frame, where drivers live,
/// answer an action at all.
///
/// `None` means "no immediate-mode node at this path" — the caller falls
/// through to the retained-`External` branch rather than treating it as a
/// failure.
fn invoke_immediate_at(
    scene: &Scene,
    source: SceneSource,
    raw_path: &str,
    args: &IntrospectValue,
) -> Option<Result<(IntrospectValue, AnswerOrigin), InvokeRefusal>> {
    let Ok((scene_segments, action_path)) = resolve_external_path(raw_path) else {
        return None;
    };
    let Some(Scene::ImmediateModeNode(node)) = lookup_addressed(scene, &scene_segments) else {
        return None;
    };
    // R1487 — the walk has landed on a driver, so every outcome from here
    // names it, refusal included.
    let origin = AnswerOrigin::of(source, true);
    let mut driver = node.handle.borrow_mut();
    let Some(intro) = driver.introspect_mut() else {
        return Some(Err(InvokeRefusal::from_surface(
            InvokeError::IntrospectionOptedOut,
            origin,
        )));
    };
    Some(
        invoke_declared(intro, &action_path, args.clone())
            .map(|value| (value, origin))
            .map_err(|(err, announced)| {
                InvokeRefusal::from_surface(err, origin).announcing(announced)
            }),
    )
}

/// R1481 §2 #4 §5.12 — invoke against a scene held by shared reference.
///
/// The paint-scene entry point: `scene/invoke` reaches for this after the
/// state scene reports no external, exactly as `scene/query` does. Only
/// immediate-mode drivers answer here, and that restriction is the honest
/// one rather than a shortcut — a retained `ExternalNode` in a painted frame
/// is a `Box` the view fn rebuilds every frame, so a write into it would be
/// discarded before the next paint. Refusing is the truthful answer for that
/// shape; a driver's `Rc` is the same one the game loop ticks, so acting on
/// it is not.
///
/// # Errors
///
/// [`InvokeError::NoExternalAtPath`] when the path reaches no immediate-mode
/// driver, plus the usual action-channel failures.
pub fn invoke_shared(
    scene: &Scene,
    raw_path: &str,
    args: &IntrospectValue,
) -> Result<IntrospectValue, InvokeError> {
    invoke_shared_from(scene, SceneSource::Paint, raw_path, args)
        .map(|(value, _)| value)
        .map_err(|refusal| refusal.error)
}

/// R1487 §2 #4 §5.12 §2 #7 — [`invoke_shared`] reporting [which surface
/// acted or refused](AnswerOrigin).
///
/// The origin-disclosing peer of [`invoke_from`] on the shared-borrow side,
/// and the site where the write channel stops denying what the read
/// resolves: when no immediate-mode driver is at the address, the walk now
/// **looks** for a retained `External` there — with
/// [`crate::resolve::external_node_at`], the same
/// resolution `scene/query` uses, so the two channels cannot disagree about
/// what exists. Finding one yields
/// [`RetainedNodeNotWritable`](InvokeError::RetainedNodeNotWritable) named
/// by its surface; finding nothing yields the unreached
/// [`NoExternalAtPath`](InvokeError::NoExternalAtPath) that word has always
/// meant.
///
/// # Errors
///
/// See [`invoke_shared`]; the refusal additionally names the surface when
/// the walk identified one.
pub fn invoke_shared_from(
    scene: &Scene,
    source: SceneSource,
    raw_path: &str,
    args: &IntrospectValue,
) -> Result<(IntrospectValue, AnswerOrigin), InvokeRefusal> {
    if let Some(result) = invoke_immediate_at(scene, source, raw_path, args) {
        return result;
    }
    Err(retained_refusal(scene, source, raw_path))
}

/// R1487 — classify a shared-walk miss: a retained surface that cannot be
/// acted on, or genuinely nothing at the address.
fn retained_refusal(scene: &Scene, source: SceneSource, raw_path: &str) -> InvokeRefusal {
    let Ok((scene_segments, _)) = resolve_external_path(raw_path) else {
        return InvokeRefusal::unreached(InvokeError::NoExternalAtPath);
    };
    if external_node_at(scene, &scene_segments).is_some() {
        return InvokeRefusal::from_surface(
            InvokeError::RetainedNodeNotWritable,
            AnswerOrigin::of(source, false),
        );
    }
    InvokeRefusal::unreached(InvokeError::NoExternalAtPath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, ReadRefusal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

    use pinion_core::external::{InterveneError, IntrospectSchema, SchemaArg, SchemaField};

    /// A surface declaring BOTH channels plus a parametric family — the peer of
    /// `intervene`'s fixture, restated here because a test that shared it would
    /// stop proving the two dispatchers agree by construction.
    const MIXED: IntrospectSchema = IntrospectSchema::new(
        const {
            &[
                SchemaField::new("depth", "int"),
                SchemaField::parametric(
                    "cell.<row>",
                    "int",
                    const { &[SchemaArg::index("row", "depth")] },
                ),
                SchemaField::action("reset", "string"),
                // Declared and deliberately NOT dispatched — the surface
                // inconsistency R1637 gave its own word to. A fixture rather
                // than a real widget because the whole point of the round is
                // that no real one may stay in this state.
                SchemaField::action("promised", "string"),
            ]
        },
    );

    /// The `MIXED` surface, wired the way a real one is: a hand-written `match`
    /// beside a hand-written declaration, disagreeing in **both** directions.
    #[derive(Debug, Default)]
    struct MixedSurface {
        fired: Vec<String>,
        verdict: Option<TraitInvokeError>,
    }

    impl ExternalIntrospect for MixedSurface {
        fn schema(&self) -> IntrospectSchema {
            MIXED
        }
        fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
            (path == "depth")
                .then_some(IntrospectValue::Int(3))
                .ok_or(ReadRefusal::UnknownPath)
        }
        fn intervene(&mut self, _path: &str, _v: IntrospectValue) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            path: &str,
            _args: IntrospectValue,
        ) -> Result<IntrospectValue, TraitInvokeError> {
            if let Some(err) = self.verdict.clone() {
                return Err(err);
            }
            match path {
                // Declared, and answers.
                "reset" => {
                    self.fired.push(path.to_owned());
                    Ok(IntrospectValue::Text("reset".to_owned()))
                }
                // R1631's shape: implemented, never declared. Before R1637 this
                // arm was reachable over the wire and nothing could see it.
                "smuggled" => {
                    self.fired.push(path.to_owned());
                    Ok(IntrospectValue::Text("smuggled".to_owned()))
                }
                _ => Err(TraitInvokeError::UnknownPath),
            }
        }
    }

    impl pinion_core::external::External for MixedSurface {
        fn backends(&self) -> pinion_core::external::BackendSupport {
            pinion_core::external::BackendSupport::new(
                &[pinion_core::external::Backend::Rpc],
                pinion_core::external::BackendFallback::Skip,
            )
        }
        fn repaint_ownership(&self) -> pinion_core::external::RepaintOwner {
            pinion_core::external::RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> pinion_core::external::ThreadOwnership {
            pinion_core::external::ThreadOwnership::UiThreadSync
        }
        fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
            Some(self)
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    fn mixed_scene() -> Scene {
        Scene::External(ExternalNode::new(Box::new(MixedSurface::default())))
    }

    /// R1566 §2 #7 — a call addressed at a declared SLOT is told what the name
    /// is, not that it is absent. R1637 — judged **before** dispatch, so the
    /// surface never sees the call.
    #[test]
    fn r1566_a_declared_slot_is_named_as_one_to_a_caller() {
        let mut surface = MixedSurface::default();
        let judge = |s: &mut MixedSurface, path: &str| {
            invoke_declared(s, path, IntrospectValue::Null)
                .unwrap_err()
                .0
        };
        assert_eq!(judge(&mut surface, "depth"), InvokeError::PathIsAReadSlot);
        assert_eq!(judge(&mut surface, "cell.3"), InvokeError::PathIsAReadSlot);
        assert_eq!(
            judge(&mut surface, "nothing_declared"),
            InvokeError::UnknownInvokePath
        );
        // R1637 — the two facts R1566 fused now differ: a name the surface
        // published and did not answer is a bug in the SURFACE, and a name
        // nobody published is a mistake by the CALLER.
        assert_eq!(
            judge(&mut surface, "promised"),
            InvokeError::DeclaredButUnhandled
        );
        // None of the four reached the impl.
        assert!(surface.fired.is_empty(), "fired: {:?}", surface.fired);
    }

    /// R1637 §2 #2 — the round's whole point: an action a surface implements and
    /// never declares is **not callable**, and the impl is not even reached.
    ///
    /// The counterfactual is the pre-R1637 tree, where this call succeeded.
    #[test]
    fn r1637_an_undeclared_action_is_not_on_the_wire() {
        let mut scene = mixed_scene();
        // Declared: it answers.
        assert_eq!(
            invoke(&mut scene, "/external/reset", IntrospectValue::Null).unwrap(),
            IntrospectValue::Text("reset".to_owned()),
        );
        // Implemented, undeclared: refused with the caller's word.
        assert_eq!(
            invoke(&mut scene, "/external/smuggled", IntrospectValue::Null).unwrap_err(),
            InvokeError::UnknownInvokePath,
        );
        // And `$schema` agrees with the wire — which is the property that makes
        // the refusal actionable rather than merely restrictive.
        let schema = crate::query::query(&scene, "/external/$schema").unwrap();
        let IntrospectValue::Json(serde_json::Value::Array(fields)) = schema else {
            panic!("$schema must render an array");
        };
        let names: Vec<&str> = fields
            .iter()
            .filter(|f| f["channel"] == "invoke")
            .filter_map(|f| f["path"].as_str())
            .collect();
        assert_eq!(names, ["reset", "promised"]);
    }

    /// The surface's own verdicts survive the re-judgement — including R1564's
    /// sentence, which is the one payload this transport does not author.
    #[test]
    fn r1566_a_surface_that_judged_the_call_keeps_its_verdict() {
        let judge = |err: TraitInvokeError| {
            let mut surface = MixedSurface {
                verdict: Some(err),
                ..MixedSurface::default()
            };
            invoke_declared(&mut surface, "reset", IntrospectValue::Null)
                .unwrap_err()
                .0
        };
        assert_eq!(
            judge(TraitInvokeError::TypeMismatch),
            InvokeError::InvokeTypeMismatch
        );
        let refused = judge(TraitInvokeError::rejected("no detector installed"));
        let InvokeError::InvokeRejected(reason) = refused else {
            panic!("R1564's sentence must survive: {refused:?}");
        };
        assert_eq!(reason.as_str(), "no detector installed");
    }

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    // R1481 §2 #4 §5.12 — a live immediate-mode driver is writable.
    #[derive(Debug)]
    struct SpeedDriver {
        speed: i64,
        opted_in: bool,
    }

    impl pinion_core::scene::ImmediateMode for SpeedDriver {
        fn introspect(&self) -> Option<&dyn pinion_core::external::ExternalIntrospect> {
            self.opted_in.then_some(self)
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn pinion_core::external::ExternalIntrospect> {
            self.opted_in.then_some(self)
        }
    }

    impl pinion_core::external::ExternalIntrospect for SpeedDriver {
        fn schema(&self) -> pinion_core::external::IntrospectSchema {
            pinion_core::external::IntrospectSchema::new(
                const {
                    &[
                        pinion_core::external::SchemaField::new("speed", "int"),
                        pinion_core::external::SchemaField::action("halve", "int"),
                    ]
                },
            )
        }
        fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
            (path == "speed")
                .then_some(IntrospectValue::Int(self.speed))
                .ok_or(ReadRefusal::UnknownPath)
        }
        fn intervene(
            &mut self,
            path: &str,
            value: IntrospectValue,
        ) -> Result<(), pinion_core::external::InterveneError> {
            match (path, value) {
                ("speed", IntrospectValue::Int(n)) => {
                    self.speed = n;
                    Ok(())
                }
                ("speed", _) => Err(pinion_core::external::InterveneError::TypeMismatch),
                _ => Err(pinion_core::external::InterveneError::UnknownPath),
            }
        }
        fn invoke(
            &mut self,
            path: &str,
            _args: IntrospectValue,
        ) -> Result<IntrospectValue, pinion_core::external::InvokeError> {
            match path {
                "halve" => {
                    self.speed /= 2;
                    Ok(IntrospectValue::Int(self.speed))
                }
                _ => Err(pinion_core::external::InvokeError::UnknownPath),
            }
        }
    }

    fn driver_scene(speed: i64, opted_in: bool) -> Scene {
        Scene::Container(pinion_core::scene::ContainerNode::new(vec![
            Scene::ImmediateModeNode(
                pinion_core::scene::ImmediateModeNode::from_driver(
                    SpeedDriver { speed, opted_in },
                    pinion_core::scene::Rect::default(),
                )
                .with_tag("ball".to_owned()),
            ),
        ]))
    }

    #[test]
    fn r1481_a_live_driver_answers_an_action_through_a_shared_scene() {
        // The defect this closes: `scene/query ball/external/velocity`
        // answered a number while `scene/invoke` on the same path answered
        // `NoExternalAtPath` — a refusal the read had just disproved.
        let scene = driver_scene(10, true);
        let out = invoke_shared(&scene, "/ball/external/halve", &IntrospectValue::Null).unwrap();
        assert_eq!(out, IntrospectValue::Int(5));
        // The write landed on the SAME driver the tick loop holds, which is
        // the whole reason a shared `&Scene` is enough here.
        assert_eq!(
            crate::query::query(&scene, "/ball/external/speed").unwrap(),
            IntrospectValue::Int(5),
        );
    }

    #[test]
    fn r1481_a_driver_that_opted_out_says_so_rather_than_denying_it_exists() {
        let scene = driver_scene(10, false);
        let err =
            invoke_shared(&scene, "/ball/external/halve", &IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::IntrospectionOptedOut);
    }

    #[test]
    fn r1481_a_path_reaching_no_driver_is_still_refused() {
        // The shared entry point does NOT fall back to retained externals:
        // a painted `ExternalNode` is a per-frame `Box`, so a write into it
        // would be discarded before the next paint.
        //
        // R1487 — the refusal is unchanged; the WORD is. Pre-R1487 this read
        // `NoExternalAtPath` about an address the read below resolves, which
        // is a statement about the scene that is not true (§2 #7).
        let scene = counted_scene(1);
        let err =
            invoke_shared(&scene, "/external/increment", &IntrospectValue::Int(1)).unwrap_err();
        assert_eq!(err, InvokeError::RetainedNodeNotWritable);
        assert!(
            crate::query::query(&scene, "/external/count").is_ok(),
            "the read resolves this address, so the write may not deny it exists",
        );
    }

    #[test]
    fn r1487_a_retained_refusal_names_the_surface_it_reached() {
        // The other half of the same fact: the refusal is attributable. A
        // caller that asked which surface turned it down learns `paint_frame`
        // — the very word the READ reports for this node — instead of
        // learning nothing.
        let scene = counted_scene(1);
        let refusal = invoke_shared_from(
            &scene,
            SceneSource::Paint,
            "/external/increment",
            &IntrospectValue::Int(1),
        )
        .expect_err("a per-frame node cannot act");
        assert_eq!(refusal.error, InvokeError::RetainedNodeNotWritable);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::PaintFrame));
        assert_eq!(
            crate::query::query_from(&scene, SceneSource::Paint, "/external/count")
                .expect("the read answers")
                .1,
            AnswerOrigin::PaintFrame,
            "the refusal names the same surface the answer does",
        );
    }

    #[test]
    fn r1487_nothing_at_the_address_still_names_nothing() {
        // The counterpart that keeps `RetainedNodeNotWritable` meaning
        // something: an address with no external really does report
        // `NoExternalAtPath`, and names no surface. Without this, "the walk
        // found a surface" would be an unfalsifiable claim.
        let scene = Scene::Container(pinion_core::scene::ContainerNode::new(vec![]));
        let refusal = invoke_shared_from(
            &scene,
            SceneSource::Paint,
            "/ghost/external/increment",
            &IntrospectValue::Int(1),
        )
        .expect_err("nothing is there");
        assert_eq!(refusal.error, InvokeError::NoExternalAtPath);
        assert_eq!(refusal.refused_by, None);
    }

    #[test]
    fn r1487_a_driver_action_names_the_driver() {
        // The success half. `invoke` had no way to say which of the two
        // surfaces acted, so an agent could not tell an action on the live
        // simulation from one on a copy the next paint discards.
        let scene = driver_scene(10, true);
        let (value, origin) = invoke_shared_from(
            &scene,
            SceneSource::Paint,
            "/ball/external/halve",
            &IntrospectValue::Null,
        )
        .expect("a live driver acts");
        assert_eq!(value, IntrospectValue::Int(5));
        assert_eq!(origin, AnswerOrigin::PaintDriver);
    }

    #[test]
    fn r1487_a_state_scene_action_names_the_state_scene() {
        let mut scene = counted_scene(5);
        let (value, origin) = invoke_from(
            &mut scene,
            SceneSource::State,
            "/external/increment",
            IntrospectValue::Int(3),
        )
        .expect("the model acts");
        assert_eq!(value, IntrospectValue::Int(8));
        assert_eq!(origin, AnswerOrigin::State);
    }

    #[test]
    fn r1487_an_opted_out_driver_names_itself_when_it_refuses() {
        // Reached-and-declined is a different fact from not-reached, and the
        // origin is what carries it.
        let scene = driver_scene(10, false);
        let refusal = invoke_shared_from(
            &scene,
            SceneSource::Paint,
            "/ball/external/halve",
            &IntrospectValue::Null,
        )
        .expect_err("the driver exposes nothing");
        assert_eq!(refusal.error, InvokeError::IntrospectionOptedOut);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::PaintDriver));
    }

    #[test]
    fn r1487_an_undeclared_action_on_a_reached_surface_names_it() {
        let mut scene = counted_scene(1);
        let refusal = invoke_from(
            &mut scene,
            SceneSource::State,
            "/external/nope",
            IntrospectValue::Null,
        )
        .expect_err("the action is not declared");
        assert_eq!(refusal.error, InvokeError::UnknownInvokePath);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::State));
    }

    #[test]
    fn counted_increment_returns_new_total() {
        let mut scene = counted_scene(5);
        let result = invoke(&mut scene, "/external/increment", IntrospectValue::Int(3)).unwrap();
        assert_eq!(result, IntrospectValue::Int(8));
    }

    #[test]
    fn counted_increment_via_window_prefix() {
        let mut scene = counted_scene(0);
        let result = invoke(
            &mut scene,
            "/window[main]/external/increment",
            IntrospectValue::Int(7),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(7));
    }

    #[test]
    fn r1303_no_primary_invoke_by_tag_mutates_the_correct_extra() {
        // R1303 PR-51 §2 #2 — a no-primary binding's state scene is
        // `Container([extra, extra])`. The audit's concern was that a
        // MUTATING method (invoke/intervene) on the bare path could
        // misroute to the wrong (first) extra. The stable contract: invoke
        // addressed by the extra's explicit tag mutates THAT extra. Here
        // `increment` on pane_b (start 202) by 5 must return 207, and
        // pane_a (start 101) must be untouched.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode};
        let a =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(101))).with_tag("pane_a"));
        let b =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(202))).with_tag("pane_b"));
        let mut c = ContainerNode::new(vec![a, b]).without_primary_head();
        c.rect = Rect::new(0, 0, 100, 100);
        let mut scene = Scene::Container(c);
        let result = invoke(
            &mut scene,
            "/pane_b/external/increment",
            IntrospectValue::Int(5),
        )
        .expect("invoke by tag reaches the tagged extra");
        assert_eq!(result, IntrospectValue::Int(207));
        // pane_a untouched — the mutation did not leak to the first extra.
        let a_node = scene
            .find_external_with_tag("pane_a")
            .expect("pane_a still present");
        assert_eq!(
            a_node.handle.introspect().unwrap().query("count"),
            Ok(IntrospectValue::Int(101)),
        );
    }

    #[test]
    fn r1303_no_primary_bare_invoke_rejects() {
        // (R1307) The M1 fix on the MUTATING side: a bare `/external` invoke
        // on a no-primary-head container REJECTS with `NoExternalAtPath`
        // rather than silently mutating the first extra. `primary_external_mut`
        // returns `None` for the marked container, so the audit's silent-
        // misroute footgun on the AI mutate wire is closed. Clients address
        // extras by tag (proven in the by-tag test above).
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode};
        let a =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(101))).with_tag("pane_a"));
        let b =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(202))).with_tag("pane_b"));
        let mut c = ContainerNode::new(vec![a, b]).without_primary_head();
        c.rect = Rect::new(0, 0, 100, 100);
        let mut scene = Scene::Container(c);
        let err = invoke(&mut scene, "/external/increment", IntrospectValue::Int(6)).unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
        // The first extra was NOT mutated (still 101).
        assert_eq!(
            scene
                .find_external_with_tag("pane_a")
                .unwrap()
                .handle
                .introspect()
                .unwrap()
                .query("count"),
            Ok(IntrospectValue::Int(101)),
        );
    }

    #[test]
    fn stub_at_root_reports_introspection_opted_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = invoke(&mut scene, "/external/anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::IntrospectionOptedOut);
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = invoke(&mut scene, "/external/x", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let mut scene = counted_scene(0);
        let err = invoke(&mut scene, "/some/other/shape", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, InvokeError::UnsupportedPath);
    }

    #[test]
    fn unknown_action_path_propagates() {
        let mut scene = counted_scene(0);
        let err = invoke(&mut scene, "/external/ghost", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::UnknownInvokePath);
    }

    #[test]
    fn type_mismatch_propagates() {
        let mut scene = counted_scene(0);
        let err = invoke(
            &mut scene,
            "/external/increment",
            IntrospectValue::Text("nope".to_string()),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::InvokeTypeMismatch);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let mut scene = counted_scene(0);
        let err = invoke(
            &mut scene,
            "/window[main/external/increment",
            IntrospectValue::Int(1),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::Path(PathError::MalformedPrefix));
    }

    // ---- R666 §5.34 v1: multi-External addressing ----

    fn container_with_nested_counted(tag: &'static str, count: i64) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let ext =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag));
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    fn container_with_two_counted_siblings(
        primary_tag: &'static str,
        primary_count: i64,
        extra_tag: &'static str,
        extra_count: i64,
    ) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let primary = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(primary_count))).with_tag(primary_tag),
        );
        let extra = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(extra_count))).with_tag(extra_tag),
        );
        let mut c = ContainerNode::new(vec![primary, extra]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r666_invoke_nested_external_by_tag() {
        // /counter/external/increment walks Container.children by tag,
        // finds the External with that tag, calls increment on it.
        let mut scene = container_with_nested_counted("counter", 0);
        let result = invoke(
            &mut scene,
            "/counter/external/increment",
            IntrospectValue::Int(5),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(5));
    }

    #[test]
    fn r666_invoke_extra_external_by_composite_tag() {
        // R55.D.5 multi-External shape: the extra sibling carries a
        // composite tag (`todo_toggle#1` convention). v1 path syntax
        // addresses it directly without disturbing the primary.
        let mut scene = container_with_two_counted_siblings("todo_list", 100, "todo_toggle#1", 0);
        let result = invoke(
            &mut scene,
            "/todo_toggle#1/external/increment",
            IntrospectValue::Int(3),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(3));
        // Primary untouched — v0 fallback still resolves to it.
        let primary_via_v0 =
            invoke(&mut scene, "/external/increment", IntrospectValue::Int(0)).unwrap();
        assert_eq!(primary_via_v0, IntrospectValue::Int(100));
    }

    #[test]
    fn r666_invoke_extra_external_with_window_prefix() {
        let mut scene = container_with_two_counted_siblings("primary", 0, "todo_delete#7", 0);
        let result = invoke(
            &mut scene,
            "/window[main]/todo_delete#7/external/increment",
            IntrospectValue::Int(11),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(11));
    }

    #[test]
    fn r666_invoke_unknown_segment_is_no_external() {
        let mut scene = container_with_nested_counted("counter", 0);
        let err = invoke(
            &mut scene,
            "/ghost/external/increment",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
    }

    #[test]
    fn r666_invoke_non_external_target_is_no_external() {
        use pinion_core::scene::{BoxNode as BNode, ContainerNode, Rect};
        let child = Scene::Box(BNode::filled(Rect::default(), Color::default()).with_tag("info"));
        let mut c = ContainerNode::new(vec![child]);
        c.rect = Rect::new(0, 0, 100, 100);
        let mut scene = Scene::Container(c);
        let err = invoke(
            &mut scene,
            "/info/external/increment",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
    }
}
