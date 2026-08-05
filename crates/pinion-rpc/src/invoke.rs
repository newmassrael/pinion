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
//!      [`ExternalIntrospect::invoke`](pinion_core::external::ExternalIntrospect::invoke) action channel.
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

use pinion_core::Scene;
use pinion_core::external::{IntrospectValue, InvokeError as TraitInvokeError, RefusalReason};

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
    /// The action path is not declared in the `External`'s schema.
    UnknownInvokePath,
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

impl From<TraitInvokeError> for InvokeError {
    fn from(err: TraitInvokeError) -> Self {
        // `TraitInvokeError` is `#[non_exhaustive]`, so the wildcard is
        // mandatory here. R1564 — it no longer answers `InvokeRejected`: see
        // [`UnmappedSurfaceError`](InvokeError::UnmappedSurfaceError) for why a
        // reason-carrying variant must not be the wildcard's landing site.
        match err {
            TraitInvokeError::UnknownPath => InvokeError::UnknownInvokePath,
            TraitInvokeError::TypeMismatch => InvokeError::InvokeTypeMismatch,
            TraitInvokeError::Rejected(reason) => InvokeError::InvokeRejected(reason),
            _ => InvokeError::UnmappedSurfaceError,
        }
    }
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
    intro
        .invoke(&action_path, args)
        .map(|value| (value, origin))
        .map_err(|e| InvokeRefusal::from_surface(e.into(), origin))
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
        intro
            .invoke(&action_path, args.clone())
            .map(|value| (value, origin))
            .map_err(|e| InvokeRefusal::from_surface(e.into(), origin)),
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
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

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
                const { &[pinion_core::external::SchemaField::new("speed", "int")] },
            )
        }
        fn query(&self, path: &str) -> Option<IntrospectValue> {
            (path == "speed").then_some(IntrospectValue::Int(self.speed))
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
            Some(IntrospectValue::Int(101)),
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
            Some(IntrospectValue::Int(101)),
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
