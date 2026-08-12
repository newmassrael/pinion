//! `scene/intervene` RPC method dispatch — R56.1.f.3 §5.22 substrate
//! gap (Round 17 8 typed methods → 9 with this slice). Symmetric peer
//! of [`crate::invoke`](fn@crate::invoke) for the read-write state-mutation side door
//! defined by [`ExternalIntrospect::intervene`](pinion_core::external::ExternalIntrospect::intervene) (§5.15 item 7).
//!
//! Wires the same three pieces as [`crate::query`](fn@crate::query) / [`crate::invoke`](fn@crate::invoke):
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>`
//!      with single-window short-circuit.
//!   2. **§5.2 scene tree walk** — R666 §5.34 v1 multi-External
//!      addressing via `path::split_at_external` +
//!      [`Scene::lookup_path_mut`] + [`Scene::primary_external_mut`].
//!   3. **§5.15 item 7 intervene dispatch** — descend through
//!      [`External::introspect_mut`](pinion_core::external::External::introspect_mut) and consult the
//!      [`ExternalIntrospect::intervene`](pinion_core::external::ExternalIntrospect::intervene) write channel.
//!
//! v1 scene-path syntax accepted (mirror of [`crate::invoke`](fn@crate::invoke)):
//!
//!   * `/external/<state>` — primary External at root (v0 retained).
//!   * `/<tag>/external/<state>` — DFS lookup by `ExternalNode` tag,
//!     composite tags (`todo_toggle#1`) pass through verbatim.
//!   * `/<seg>/.../<tag>/external/<state>` — nested Container walk.
//!
//! Other shapes return [`InterveneError::UnsupportedPath`].
//!
//! Where [`crate::invoke`](fn@crate::invoke) is the "call an action and get a value
//! back" surface (W3C `RPC.call`-mirror), [`intervene`] is the
//! "write a value to a state path" surface (W3C `RPC.set`-mirror).
//! `TextField.caret = 4` flows through this path; `TextField.send
//! Focus` flows through [`crate::invoke`](fn@crate::invoke). R56.1.f.3 motivated
//! landing it: the §5.22 selection sidecar exposes both a query path
//! (`/external/selection` → Json) and an intervene path (Json or
//! Null) on `TextFieldExternal`, but the RPC wiring side was missing
//! a wire-form entry point for the mutation half.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is handled by
//! [`crate::dispatch`](fn@crate::dispatch); this module exposes the typed dispatcher only.

use pinion_core::Scene;
use pinion_core::external::{
    InterveneError as TraitInterveneError, IntrospectSchema, IntrospectValue, RefusalReason,
    SchemaChannel,
};

use crate::origin::{AnswerOrigin, Refusal, SceneSource};
use crate::path::PathError;
use crate::resolve::{
    ResolveExternalError, external_node_at, lookup_addressed, resolve_external_introspect_mut,
    resolve_external_path,
};

/// Reasons the typed [`intervene`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterveneError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Path expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// The `External` did not opt in to §5.15 item 7 introspection
    /// (so the intervene channel is unreachable).
    IntrospectionOptedOut,
    /// The state path is not declared in the `External`'s schema **on any
    /// channel**.
    ///
    /// R1566 narrowed this from "the surface's `intervene` did not recognise
    /// the string", which is a different fact and was frequently a false one —
    /// see `InterveneError::from_declined` (crate-private).
    UnknownIntervenePath,
    /// R1566 §2 #7 — the path is declared, on the **invoke** channel: it is an
    /// action to call, not a slot to write.
    ///
    /// The toolkit has no answer here at all. `setProperty()` returns a bare `bool`, so a
    /// toolkit caller who addressed a method name as a property learns only
    /// that it did not work, and has to go back to the meta-object and search
    /// `method()` themselves to find out why.
    PathIsAnAction,
    /// The value variant does not match the slot's declared type.
    InterveneTypeMismatch,
    /// The slot is read-only on this `External`. Distinct from
    /// `UnknownIntervenePath` so a client can tell "schema says this
    /// exists but you cannot write it" from "schema does not declare
    /// this slot".
    ReadOnly,
    /// The value is the right variant but its content is outside the
    /// slot's accepted range (e.g. negative caret position).
    ///
    /// R1565 §5.15 (PINION-PR82) — carries the producer's own
    /// [`RefusalReason`], forwarded verbatim to the JSON-RPC `error.data`,
    /// because the variant does not determine the range. It is the only arm of
    /// the trait-level `InterveneError` that gained one, and the only one here
    /// that needed to: `ReadOnly` and `InterveneTypeMismatch` are each fully
    /// determined by their variant, so a sentence would restate them.
    OutOfRange(RefusalReason),
    /// R1565 — the surface reported a [`TraitInterveneError`] this transport
    /// has not been taught to name. The peer of
    /// [`InvokeError::UnmappedSurfaceError`](crate::InvokeError::UnmappedSurfaceError),
    /// added for the same reason and by the same correction: the wildcard's
    /// previous landing site was `InterveneTypeMismatch`, which tells a client
    /// something specific and probably untrue about its own payload.
    ///
    /// Unreachable today by construction.
    UnmappedSurfaceError,
    /// R1487 §2 #7 — the address named a **retained** `External` reached
    /// through the shared ([`intervene_shared`]) walk, which cannot write
    /// to it. The peer of
    /// [`InvokeError::RetainedNodeNotWritable`](crate::InvokeError::RetainedNodeNotWritable);
    /// see it for why the refusal is right and the word it used to carry
    /// was not.
    RetainedNodeNotWritable,
}

/// A refused [`intervene_from`], and the surface that refused it.
pub type InterveneRefusal = Refusal<InterveneError>;

impl From<PathError> for InterveneError {
    fn from(err: PathError) -> Self {
        InterveneError::Path(err)
    }
}

impl From<ResolveExternalError> for InterveneError {
    fn from(err: ResolveExternalError) -> Self {
        match err {
            ResolveExternalError::Path(e) => InterveneError::Path(e),
            ResolveExternalError::UnsupportedPath => InterveneError::UnsupportedPath,
            ResolveExternalError::NoExternalAtPath => InterveneError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => InterveneError::IntrospectionOptedOut,
        }
    }
}

impl InterveneError {
    /// R1566 §2 #7 §5.12 — the wire refusal for a surface that declined a
    /// write, judged **against that surface's own declaration**.
    ///
    /// # Why this is not a `From` impl
    ///
    /// It was one until R1566, and an infallible `err.into()` is exactly what
    /// made the bug possible: the conversion had no access to the schema, so
    /// the only thing it could do with the trait's `UnknownPath` was repeat it.
    /// The trait documents that variant as "path is not declared in the
    /// schema", and an impl cannot honour that — it knows only that *its own*
    /// `match` fell through. Both facts are spelled `UnknownPath`, and one of
    /// them is a lie whenever the path is declared.
    ///
    /// The lie is not hypothetical and not rare. R1353 wrote the rule down as
    /// [`read_only_or_unknown`](pinion_core::external::read_only_or_unknown),
    /// whose doc says "every read-only External needs exactly this rule" — and
    /// it is opt-in, so **two** of the framework's surfaces route through it
    /// against 97 that implement `intervene`. Measured over the wire at R1566:
    /// `hello-node-editor` answers `layout_crossings` to `query` and tells a
    /// writer it does not exist; `hello-untangle` does the same for `untangle`,
    /// which its own `$schema` declares as an action.
    ///
    /// Taking `schema` and `path` by value makes the judgement unskippable
    /// rather than remembered: there are seven dispatch sites for this
    /// conversion (`intervene` × 2, `dry_run` × 2, `simulate` × 2, `rewind`),
    /// and a rule they each had to *apply* is a rule six of them would
    /// eventually not.
    #[must_use]
    pub(crate) fn from_declined(
        err: TraitInterveneError,
        schema: &IntrospectSchema,
        path: &str,
    ) -> Self {
        match err {
            // The only arm that is re-judged, and only against what the
            // surface itself declares — this never contradicts an impl, it
            // answers where the impl had nothing to say. R1565's note below
            // stands for the rest: naming each variant explicitly leaves the
            // wildcard for the genuinely unknown.
            TraitInterveneError::UnknownPath => match schema.field_for(path) {
                Some(field) if field.channel == SchemaChannel::Invoke => {
                    InterveneError::PathIsAnAction
                }
                // A SCALAR declared path the impl did not recognise is a path
                // the impl does not write, so `ReadOnly` is the true statement.
                //
                // A PARAMETRIC one is not, and the difference is the round's
                // one sharp edge. `SchemaChannel::Read` does not mean
                // "read-only" — it means "readable", and every writable slot is
                // declared that way too. For a scalar that costs nothing,
                // because an impl that writes a name does not answer
                // `UnknownPath` for it. For a family it costs everything: the
                // impl DID recognise the shape and rejected the ARGUMENT, so
                // `voice.999.gain` under a writable `voice.<id>.gain` would be
                // reported read-only, which is false about a family a client
                // may write all day. `hello-audio-engine` caught exactly that.
                //
                // Nothing is lost by stopping here: `read_only_or_unknown` has
                // answered `ReadOnly` for a genuinely read-only family since
                // R1353, at the impl, where the difference is knowable.
                Some(field) if field.args.is_empty() => InterveneError::ReadOnly,
                Some(_) | None => InterveneError::UnknownIntervenePath,
            },
            TraitInterveneError::TypeMismatch => InterveneError::InterveneTypeMismatch,
            TraitInterveneError::ReadOnly => InterveneError::ReadOnly,
            TraitInterveneError::OutOfRange(reason) => InterveneError::OutOfRange(reason),
            // R1565 — the wildcard no longer absorbs `TypeMismatch`. It used
            // to, which meant a variant added upstream would be reported to a
            // client as "your value was the wrong TYPE" — a specific and
            // probably false statement about the caller's payload.
            _ => InterveneError::UnmappedSurfaceError,
        }
    }
}

/// Resolve `raw_path` against `scene` and write `value` to the slot
/// there.
///
/// See module docs for the v0 path syntax. The `scene` reference is
/// borrowed mutably for the lifetime of the call so the underlying
/// `External` can update its state.
///
/// # Errors
///
/// Returns [`InterveneError`] when the path is malformed, the scene
/// root does not match the path shape, or the underlying `External`
/// rejects the write (unknown path, type mismatch, read-only slot,
/// or out-of-range value).
pub fn intervene(
    scene: &mut Scene,
    raw_path: &str,
    value: IntrospectValue,
) -> Result<(), InterveneError> {
    intervene_from(scene, SceneSource::State, raw_path, value)
        .map(|((), _origin)| ())
        .map_err(|refusal| refusal.error)
}

/// Resolve `raw_path` against `scene`, write `value` to the slot there, and
/// report [which surface took the write](AnswerOrigin).
///
/// `source` names the scene the caller is handing over. [`intervene`] is
/// the projection that drops the origin — the same pairing
/// [`crate::query`](fn@crate::query) / [`crate::query_from`] have had since
/// R1482.
///
/// # Errors
///
/// Returns an [`InterveneRefusal`] carrying the same [`InterveneError`]
/// [`intervene`] reports, plus the surface that produced it when the walk
/// identified one.
pub fn intervene_from(
    scene: &mut Scene,
    source: SceneSource,
    raw_path: &str,
    value: IntrospectValue,
) -> Result<((), AnswerOrigin), InterveneRefusal> {
    // R667 §5.34 — `/window[id]/<segs>/external/<state>` parse +
    // Container/Scroll walk + multi-widget primary descent + §5.15
    // introspect-mut lookup lifted into [`resolve_external_introspect_mut`].
    if let Some(result) = intervene_immediate_at(scene, source, raw_path, &value) {
        return result;
    }
    // R1487 — bound once, before the branches; see [`crate::invoke_from`].
    let origin = AnswerOrigin::of(source, false);
    let (intro, state_path) = resolve_external_introspect_mut(scene, raw_path)
        .map_err(|e| InterveneRefusal::from_resolve(e, origin))?;
    match intro.intervene(&state_path, value) {
        Ok(()) => Ok(((), origin)),
        // R1566 — the declaration is read only on the REFUSAL path, and after
        // the write: after, because that is when the mutable borrow ends and
        // because a surface may declare a path it grew during this very call;
        // only, because a successful write must not pay for a judgement it
        // does not need.
        Err(err) => {
            let declared = intro.schema();
            Err(InterveneRefusal::from_surface(
                InterveneError::from_declined(err, &declared, &state_path),
                origin,
            ))
        }
    }
}

/// R1481 §2 #4 §5.12 — the immediate-mode half of [`intervene`], reachable
/// through a **shared** `&Scene`. The write mirror of `query`'s R828 branch;
/// see [`crate::invoke::invoke_shared`] for why the borrow seam that deferred
/// it dissolves once the work happens inside the borrow.
///
/// `None` means "no immediate-mode node here" — fall through to the retained
/// branch rather than failing.
fn intervene_immediate_at(
    scene: &Scene,
    source: SceneSource,
    raw_path: &str,
    value: &IntrospectValue,
) -> Option<Result<((), AnswerOrigin), InterveneRefusal>> {
    let Ok((scene_segments, state_path)) = resolve_external_path(raw_path) else {
        return None;
    };
    let Some(Scene::ImmediateModeNode(node)) = lookup_addressed(scene, &scene_segments) else {
        return None;
    };
    let origin = AnswerOrigin::of(source, true);
    let mut driver = node.handle.borrow_mut();
    let Some(intro) = driver.introspect_mut() else {
        return Some(Err(InterveneRefusal::from_surface(
            InterveneError::IntrospectionOptedOut,
            origin,
        )));
    };
    Some(match intro.intervene(&state_path, value.clone()) {
        Ok(()) => Ok(((), origin)),
        Err(err) => {
            let declared = intro.schema();
            Err(InterveneRefusal::from_surface(
                InterveneError::from_declined(err, &declared, &state_path),
                origin,
            ))
        }
    })
}

/// R1481 §2 #4 §5.12 — intervene against a scene held by shared reference.
/// The paint-scene entry point; only immediate-mode drivers answer, for the
/// reason given on [`crate::invoke::invoke_shared`].
///
/// # Errors
///
/// [`InterveneError::NoExternalAtPath`] when the path reaches no
/// immediate-mode driver, plus the usual write-channel failures.
pub fn intervene_shared(
    scene: &Scene,
    raw_path: &str,
    value: &IntrospectValue,
) -> Result<(), InterveneError> {
    intervene_shared_from(scene, SceneSource::Paint, raw_path, value)
        .map(|((), _origin)| ())
        .map_err(|refusal| refusal.error)
}

/// R1487 §2 #4 §5.12 §2 #7 — [`intervene_shared`] reporting [which surface
/// took the write or refused it](AnswerOrigin).
///
/// Mirror of [`crate::invoke::invoke_shared_from`], including the half that
/// stops the write channel denying an address the read resolves: a retained
/// `External` found here refuses as
/// [`RetainedNodeNotWritable`](InterveneError::RetainedNodeNotWritable),
/// named by its surface, instead of as "there is no external at that path".
///
/// # Errors
///
/// See [`intervene_shared`]; the refusal additionally names the surface when
/// the walk identified one.
pub fn intervene_shared_from(
    scene: &Scene,
    source: SceneSource,
    raw_path: &str,
    value: &IntrospectValue,
) -> Result<((), AnswerOrigin), InterveneRefusal> {
    if let Some(result) = intervene_immediate_at(scene, source, raw_path, value) {
        return result;
    }
    Err(retained_refusal(scene, source, raw_path))
}

/// R1487 — classify a shared-walk miss; mirror of
/// [`crate::invoke`](mod@crate::invoke)'s.
fn retained_refusal(scene: &Scene, source: SceneSource, raw_path: &str) -> InterveneRefusal {
    let Ok((scene_segments, _)) = resolve_external_path(raw_path) else {
        return InterveneRefusal::unreached(InterveneError::NoExternalAtPath);
    };
    if external_node_at(scene, &scene_segments).is_some() {
        return InterveneRefusal::from_surface(
            InterveneError::RetainedNodeNotWritable,
            AnswerOrigin::of(source, false),
        );
    }
    InterveneRefusal::unreached(InterveneError::NoExternalAtPath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, ReadRefusal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

    use pinion_core::external::{SchemaArg, SchemaField};

    /// A surface declaring BOTH channels plus a parametric family — the three
    /// shapes [`InterveneError::from_declined`] tells apart.
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
            ]
        },
    );

    /// R1566 §2 #7 — a fall-through is re-judged against the DECLARATION, and
    /// the three answers are three different facts.
    #[test]
    fn r1566_a_declared_path_is_never_reported_unknown_to_a_write() {
        let judge =
            |path| InterveneError::from_declined(TraitInterveneError::UnknownPath, &MIXED, path);
        assert_eq!(judge("depth"), InterveneError::ReadOnly);
        assert_eq!(judge("reset"), InterveneError::PathIsAnAction);
        assert_eq!(
            judge("nothing_declared"),
            InterveneError::UnknownIntervenePath
        );
        // ★ A PARAMETRIC family is NOT re-judged, and the reason is the one
        // sharp edge of this round: `SchemaChannel::Read` means "readable",
        // not "read-only", so a declared family may well be writable and its
        // impl may be refusing the ARGUMENT rather than the path. Answering
        // `ReadOnly` there would be a fresh false statement — the mirror of
        // the one this round exists to remove. `hello-audio-engine`'s
        // `voice.999.gain` under a writable `voice.<id>.gain` is the case.
        assert_eq!(judge("cell.0"), InterveneError::UnknownIntervenePath);
        assert_eq!(judge("cell.999"), InterveneError::UnknownIntervenePath);
    }

    /// The other half of the same contract: where the surface DID judge, its
    /// verdict is carried through untouched. Without this the round would have
    /// traded one wrong word for a differently wrong one.
    #[test]
    fn r1566_a_surface_that_judged_the_write_keeps_its_verdict() {
        let judge = |err| InterveneError::from_declined(err, &MIXED, "depth");
        assert_eq!(
            judge(TraitInterveneError::TypeMismatch),
            InterveneError::InterveneTypeMismatch
        );
        assert_eq!(
            judge(TraitInterveneError::ReadOnly),
            InterveneError::ReadOnly
        );
        let ranged = judge(TraitInterveneError::out_of_range("depth runs 0..=8"));
        let InterveneError::OutOfRange(reason) = ranged else {
            panic!("R1565's sentence must survive the re-judgement: {ranged:?}");
        };
        assert_eq!(reason.as_str(), "depth runs 0..=8");
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
            // R1566 — `halve` is declared. It was not, and this fixture was
            // itself an instance of what the round measured: a verb the
            // surface answers and its schema does not mention, so an agent
            // reading the declaration could not find it.
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
    fn r1481_a_live_driver_accepts_a_write_through_a_shared_scene() {
        let scene = driver_scene(10, true);
        intervene_shared(&scene, "/ball/external/speed", &IntrospectValue::Int(42)).unwrap();
        assert_eq!(
            crate::query::query(&scene, "/ball/external/speed").unwrap(),
            IntrospectValue::Int(42),
            "the write must reach the driver the read reports on",
        );
    }

    #[test]
    fn r1481_a_driver_write_keeps_its_typed_refusals() {
        let scene = driver_scene(10, true);
        assert_eq!(
            intervene_shared(
                &scene,
                "/ball/external/speed",
                &IntrospectValue::Text("x".into())
            )
            .unwrap_err(),
            InterveneError::InterveneTypeMismatch,
        );
        assert_eq!(
            intervene_shared(&scene, "/ball/external/ghost", &IntrospectValue::Int(1)).unwrap_err(),
            InterveneError::UnknownIntervenePath,
        );
    }

    #[test]
    fn r1481_a_path_reaching_no_driver_is_still_refused() {
        // R1487 — the refusal stands; the word it carried did not. See the
        // peer in [`crate::invoke`].
        let scene = counted_scene(1);
        assert_eq!(
            intervene_shared(&scene, "/external/count", &IntrospectValue::Int(2)).unwrap_err(),
            InterveneError::RetainedNodeNotWritable,
        );
        assert!(
            crate::query::query(&scene, "/external/count").is_ok(),
            "the read resolves this address, so the write may not deny it exists",
        );
    }

    #[test]
    fn r1487_a_retained_refusal_names_the_surface_it_reached() {
        let scene = counted_scene(1);
        let refusal = intervene_shared_from(
            &scene,
            SceneSource::Paint,
            "/external/count",
            &IntrospectValue::Int(2),
        )
        .expect_err("a per-frame node cannot take a write");
        assert_eq!(refusal.error, InterveneError::RetainedNodeNotWritable);
        assert_eq!(refusal.refused_by, Some(AnswerOrigin::PaintFrame));
    }

    #[test]
    fn r1487_nothing_at_the_address_still_names_nothing() {
        let scene = Scene::Container(pinion_core::scene::ContainerNode::new(vec![]));
        let refusal = intervene_shared_from(
            &scene,
            SceneSource::Paint,
            "/ghost/external/total",
            &IntrospectValue::Int(2),
        )
        .expect_err("nothing is there");
        assert_eq!(refusal.error, InterveneError::NoExternalAtPath);
        assert_eq!(refusal.refused_by, None);
    }

    #[test]
    fn r1487_a_write_names_the_surface_that_took_it() {
        // The success half — the fact R1482's origin word only *predicted*.
        // A write that landed can now say where, in the same reply.
        let scene = driver_scene(10, true);
        let ((), origin) = intervene_shared_from(
            &scene,
            SceneSource::Paint,
            "/ball/external/speed",
            &IntrospectValue::Int(4),
        )
        .expect("a live driver takes the write");
        assert_eq!(origin, AnswerOrigin::PaintDriver);

        let mut state = counted_scene(1);
        let ((), origin) = intervene_from(
            &mut state,
            SceneSource::State,
            "/external/count",
            IntrospectValue::Int(7),
        )
        .expect("the model takes the write");
        assert_eq!(origin, AnswerOrigin::State);
    }

    #[test]
    fn counted_set_writes_total() {
        // `CountedExternal::intervene` accepts `total = Int` and
        // overwrites the running total — the path here is the same
        // `"total"` slot `query` reports.
        let mut scene = counted_scene(5);
        intervene(&mut scene, "/external/count", IntrospectValue::Int(42)).unwrap();
        // Round-trip — invoke `total` query through the External
        // directly to confirm the write landed.
        if let Scene::External(node) = &scene {
            let intro = node.handle.introspect().expect("counted introspects");
            assert_eq!(intro.query("count"), Ok(IntrospectValue::Int(42)));
        } else {
            panic!("expected External at root");
        }
    }

    #[test]
    fn counted_set_via_window_prefix() {
        let mut scene = counted_scene(0);
        intervene(
            &mut scene,
            "/window[main]/external/count",
            IntrospectValue::Int(7),
        )
        .unwrap();
        if let Scene::External(node) = &scene {
            let intro = node.handle.introspect().expect("counted introspects");
            assert_eq!(intro.query("count"), Ok(IntrospectValue::Int(7)));
        }
    }

    #[test]
    fn stub_at_root_reports_introspection_opted_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = intervene(&mut scene, "/external/anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InterveneError::IntrospectionOptedOut);
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = intervene(&mut scene, "/external/anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InterveneError::NoExternalAtPath);
    }

    #[test]
    fn unsupported_path_shape_rejects() {
        let mut scene = counted_scene(0);
        let err = intervene(&mut scene, "/state/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, InterveneError::UnsupportedPath);
    }

    #[test]
    fn unknown_state_path_lifted_from_trait_error() {
        let mut scene = counted_scene(0);
        let err = intervene(
            &mut scene,
            "/external/unknown_slot",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InterveneError::UnknownIntervenePath);
    }

    // ---- R666 §5.34 v1: multi-External addressing ----

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

    fn read_count(scene: &Scene, tag: &str) -> i64 {
        let node = scene
            .find_external_with_tag(tag)
            .unwrap_or_else(|| panic!("no External tagged {tag}"));
        let intro = node.handle.introspect().expect("counted introspects");
        match intro.query("count").expect("count slot") {
            IntrospectValue::Int(n) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn r666_intervene_extra_external_by_composite_tag() {
        // Write only to the composite-tagged extra sibling; primary
        // untouched.
        let mut scene = container_with_two_counted_siblings("todo_list", 100, "todo_toggle#1", 0);
        intervene(
            &mut scene,
            "/todo_toggle#1/external/count",
            IntrospectValue::Int(42),
        )
        .unwrap();
        assert_eq!(read_count(&scene, "todo_toggle#1"), 42);
        assert_eq!(read_count(&scene, "todo_list"), 100);
    }

    #[test]
    fn r666_intervene_extra_external_with_window_prefix() {
        let mut scene = container_with_two_counted_siblings("primary", 0, "todo_delete#7", 0);
        intervene(
            &mut scene,
            "/window[main]/todo_delete#7/external/count",
            IntrospectValue::Int(11),
        )
        .unwrap();
        assert_eq!(read_count(&scene, "todo_delete#7"), 11);
    }

    #[test]
    fn r666_intervene_unknown_segment_is_no_external() {
        let mut scene = container_with_two_counted_siblings("a", 0, "b", 0);
        let err =
            intervene(&mut scene, "/ghost/external/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, InterveneError::NoExternalAtPath);
    }
}
