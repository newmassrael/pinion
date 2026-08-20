//! `scene/drop_targets` — **where could I drop this, and why not there**,
//! asked before anything is picked up.
//!
//! # Why this method exists
//!
//! A drag is the one gesture whose outcome is decided by a surface the person
//! is not touching yet. Every toolkit resolves it the same way — the widget
//! under the cursor accepts or ignores the event — and every toolkit therefore
//! makes the answer discoverable only by dragging something and watching.
//!
//! Measured on the reference toolkit at 6.11.1, by building a probe and running
//! it offscreen rather than by reading its documentation:
//!
//! * A widget's acceptance is **one boolean for the whole widget**, and the
//!   decision that matters lives inside an event handler that must actually
//!   run. The metaobject a generic reader can walk carries that boolean and
//!   nothing else.
//! * Per-part acceptance is a **second boolean** — a row is drop-enabled or it
//!   is not — and **names no kind at all**: a part can say YES or NO and cannot
//!   say WHAT.
//! * The members that DO name kinds are plain virtuals, absent from the
//!   metaobject, so no generic reader reaches them.
//! * A refusal is a **bare boolean**. A client that is refused learns nothing
//!   about what would have worked.
//!
//! So the useful question — *where can this land* — cannot be posed there at
//! all. §2 #2 makes an agent this framework's primary client, and an agent
//! that must start a gesture to find out whether the gesture is possible is
//! doing trial and error against a live UI.
//!
//! # What makes the answer trustworthy
//!
//! Not that a surface is honest about itself. Every judgement here runs
//! [`DropContract::admits`] over the surface's **published** declaration, and
//! that is the same call the router makes before it offers a live drag to
//! anybody: a surface is never asked about a drag its declaration excludes, and
//! never receives one it did not declare. One fact, read two ways — the
//! property [`crate::wheel_intent`] made load-bearing for the wheel at R1703,
//! applied to the drop.
//!
//! The point a row is judged at is resolved by
//! `pinion_runtime::input::drop_point_at`, which is the function
//! `InputRouter::resolve_drop_point` itself delegates to. So the part named in
//! an answer is the part a real release would land on.
//!
//! # Shape
//!
//! With no `kind` it is a census: every painted `External`, and the contract it
//! declares. That is what a screen audit wants — *what will this screen accept
//! from a drag, anywhere on it*.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/drop_targets", "id": 1 }
//! ```
//!
//! With `kind` (and optionally `action`) every row is JUDGED, so a refusal
//! carries its reason and its remedy:
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/drop_targets",
//!   "params": { "kind": "board-widget", "action": "copy" }, "id": 1 }
//! ```
//!
//! With `at: {x, y}` (or `path: "<tag>"`, resolved to that tag's painted
//! rectangle centre) it additionally answers for one point, naming the part a
//! release there would resolve to.

use pinion_core::Scene;
use pinion_core::drop_target::{DropAction, DropActions, DropContract, DropRefusal};
use pinion_core::scene::Rect;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RpcError;
use crate::resolve::painted_surfaces;

/// One clause of a surface's published contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClauseReport {
    /// The payload kind this clause admits.
    pub kind: String,
    /// What the surface can do with that kind, in wire spellings.
    pub actions: Vec<String>,
    /// The composite sub-parts the clause covers. Empty means the whole
    /// surface, which is a different claim from "no parts" and is why the
    /// [`region`](Self::region) word is published beside it.
    pub parts: Vec<String>,
    /// `"surface"` or `"parts"` — the region arm, so a reader does not have to
    /// infer it from an empty list.
    pub region: String,
}

/// How a judged surface (or point) answered.
///
/// `admits` is `None` when the call named no `kind`: the census then reports
/// what each surface declares without judging it, and a `false` there would
/// have been a claim nobody made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictReport {
    /// Whether the declaration admits the offer.
    pub admits: bool,
    /// The actions the offer and the declaration have in common, when it does.
    pub actions: Vec<String>,
    /// The refusal's one-word tag, when it does not.
    pub refused: Option<String>,
    /// The refusal as a sentence, naming what would have worked.
    pub why: Option<String>,
}

impl VerdictReport {
    fn of(result: Result<DropActions, DropRefusal>) -> Self {
        match result {
            Ok(actions) => Self {
                admits: true,
                actions: owned(&actions.wire_names()),
                refused: None,
                why: None,
            },
            Err(refusal) => Self {
                admits: false,
                actions: Vec::new(),
                refused: Some(refusal.as_wire_name().to_owned()),
                why: Some(refusal.sentence()),
            },
        }
    }
}

/// One painted `External`, its declaration, and — when a kind was named — its
/// verdict at its own painted centre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceReport {
    /// The painted `External` this row is about.
    pub surface: String,
    /// Its published clauses, in declared order.
    pub clauses: Vec<ClauseReport>,
    /// The part a release at [`asked_x`](Self::asked_x) /
    /// [`asked_y`](Self::asked_y) resolves to, `null` when the point resolves
    /// to the bare surface tag.
    ///
    /// Published because a part-scoped contract's answer depends on it, and a
    /// row that judged one part while naming none would be exactly the coarse
    /// answer this method exists to stop giving.
    pub part: Option<String>,
    /// The point the row was judged at — the surface's painted centre.
    pub asked_x: f64,
    /// The point the row was judged at.
    pub asked_y: f64,
    /// The verdict, when the call named a kind.
    pub verdict: Option<VerdictReport>,
}

/// The answer for one point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointReport {
    /// The window-logical point asked about.
    pub x: f64,
    /// The window-logical point asked about.
    pub y: f64,
    /// The surface a release there resolves to, `null` when nothing tagged
    /// covers it.
    pub surface: Option<String>,
    /// The composite sub-part a release there resolves to.
    pub part: Option<String>,
    /// The verdict, when the call named a kind.
    pub verdict: Option<VerdictReport>,
}

/// The `scene/drop_targets` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropTargetsReport {
    /// One row per painted `External`, in scene order.
    pub surfaces: Vec<SurfaceReport>,
    /// How many of them declare a contract at all.
    ///
    /// Beside the list for the reason `containment` publishes `marks`: an
    /// empty `admitting` on a screen with no declaring surface is not the same
    /// answer as an empty one on a screen with six, and a caller reading only
    /// the rows cannot tell them apart.
    pub declared: usize,
    /// How many admit the named kind. `null` when no kind was named, since
    /// nothing was judged.
    pub admitting: Option<usize>,
    /// The kind the call judged against, echoed so a stored answer says what
    /// question it answers.
    pub kind: Option<String>,
    /// The actions the call offered, echoed for the same reason.
    pub actions: Vec<String>,
    /// The per-point answer, when the call carried `at` / `path`.
    pub at: Option<PointReport>,
}

/// Why a call could not be judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetsError {
    /// `action` was not one of the three wire words.
    UnknownAction,
}

impl DropTargetsError {
    /// The word that rides in `error.data`.
    #[must_use]
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::UnknownAction => "UnknownAction",
        }
    }
}

/// The point a surface's row is judged at: its painted rectangle's centre.
///
/// The same choice `wheel_intent` made and for the same measured reason — a
/// drop verdict is a question about a POINT, because a part-scoped contract
/// answers differently on different parts of one surface. A caller wanting a
/// specific part asks with `at` / `path`.
fn centre(rect: Rect) -> (f64, f64) {
    (
        f64::from(rect.x) + f64::from(rect.w) / 2.0,
        f64::from(rect.y) + f64::from(rect.h) / 2.0,
    )
}

fn clauses_of(contract: DropContract) -> Vec<ClauseReport> {
    contract
        .clauses
        .iter()
        .map(|c| ClauseReport {
            kind: c.kind.to_owned(),
            actions: owned(&c.actions.wire_names()),
            parts: owned(c.named_parts()),
            region: if c.named_parts().is_empty() {
                "surface".to_owned()
            } else {
                "parts".to_owned()
            },
        })
        .collect()
}

/// The wire renders owned strings, not the borrowed vocabulary the contract
/// declares: a report a client can round-trip has to own what it carries.
fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

/// What the caller offered: a kind and a set of actions.
///
/// A call that names a kind but no action offers **all three**, because the
/// question then is "could this land here at all" rather than "could it land
/// here as a move". Narrowing it silently to one would have made the most
/// common call answer a stricter question than it asked.
struct Offer {
    kind: String,
    actions: DropActions,
}

/// Resolve the part a point lands on, through the router's own resolution.
fn part_at(paint: &Scene, x: f64, y: f64) -> (Option<String>, Option<String>) {
    let Some(landing) = pinion_runtime::input::drop_point_at(paint, x, y) else {
        return (None, None);
    };
    let (primary, part) = pinion_core::composite_tag::split_subindex(&landing.tag);
    (Some(primary.to_owned()), part.map(ToOwned::to_owned))
}

fn judge(
    state_scene: &Scene,
    surface: &str,
    part: Option<&str>,
    offer: Option<&Offer>,
) -> Option<VerdictReport> {
    let offer = offer?;
    let contract = pinion_runtime::input::declared_drop_contract(state_scene, surface);
    Some(VerdictReport::of(contract.admits(
        &offer.kind,
        offer.actions,
        part,
    )))
}

fn build(
    paint: &Scene,
    state_scene: &Scene,
    offer: Option<&Offer>,
    at: Option<(f64, f64)>,
) -> DropTargetsReport {
    let surfaces: Vec<SurfaceReport> = painted_surfaces(paint, state_scene)
        .into_iter()
        .map(|(tag, rect)| {
            let (x, y) = centre(rect);
            // The part is resolved at the same point the verdict is judged at,
            // through the router's own resolver, so a row cannot report a part
            // it did not judge.
            let (_, part) = part_at(paint, x, y);
            let verdict = judge(state_scene, &tag, part.as_deref(), offer);
            SurfaceReport {
                clauses: clauses_of(pinion_runtime::input::declared_drop_contract(
                    state_scene,
                    &tag,
                )),
                surface: tag,
                part,
                asked_x: x,
                asked_y: y,
                verdict,
            }
        })
        .collect();
    let declared = surfaces.iter().filter(|r| !r.clauses.is_empty()).count();
    let admitting = offer.map(|_| {
        surfaces
            .iter()
            .filter(|r| r.verdict.as_ref().is_some_and(|v| v.admits))
            .count()
    });
    let at = at.map(|(x, y)| {
        let (surface, part) = part_at(paint, x, y);
        let verdict = surface
            .as_deref()
            .and_then(|s| judge(state_scene, s, part.as_deref(), offer));
        PointReport {
            x,
            y,
            surface,
            part,
            verdict,
        }
    });
    DropTargetsReport {
        surfaces,
        declared,
        admitting,
        kind: offer.map(|o| o.kind.clone()),
        actions: offer.map_or_else(Vec::new, |o| owned(&o.actions.wire_names())),
        at,
    }
}

/// `scene/drop_targets` dispatcher entry.
///
/// A screen that has not painted answers an empty census rather than an error,
/// like [`crate::pointer_target`] and [`crate::wheel_intent`]: "no surface
/// accepts anything" is true of a screen that does not exist yet, and an error
/// would make a boot-time check impossible to run before the first frame.
///
/// # Errors
///
/// [`DropTargetsError::UnknownAction`] when `action` is not one of the three
/// wire words, and [`RpcError::internal_error`] when the report fails to
/// serialise.
pub fn handle_scene_drop_targets(
    last_paint_scene: Option<&Scene>,
    state_scene: &Scene,
    kind: Option<&str>,
    action: Option<&str>,
    at: Option<(f64, f64)>,
) -> Result<Value, RpcError> {
    let actions = match action {
        None => DropActions::one(DropAction::Copy)
            .with(DropAction::Move)
            .with(DropAction::Link),
        Some(word) => match DropAction::from_wire_name(word) {
            Some(a) => DropActions::one(a),
            None => {
                return Err(RpcError::invalid_params(
                    DropTargetsError::UnknownAction.wire_tag(),
                ));
            }
        },
    };
    let offer = kind.map(|k| Offer {
        kind: k.to_owned(),
        actions,
    });
    let report = last_paint_scene.map_or_else(
        || DropTargetsReport {
            surfaces: Vec::new(),
            declared: 0,
            admitting: offer.as_ref().map(|_| 0),
            kind: offer.as_ref().map(|o| o.kind.clone()),
            actions: offer
                .as_ref()
                .map_or_else(Vec::new, |o| owned(&o.actions.wire_names())),
            at: None,
        },
        |paint| build(paint, state_scene, offer.as_ref(), at),
    );
    serde_json::to_value(&report).map_err(|e| RpcError::internal_error(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{DropTargetsReport, handle_scene_drop_targets};
    use pinion_core::drop_target::{
        DropAction, DropActions, DropClause, DropContract, DropOffer, DropVerdict,
    };
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
        IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, ThreadOwnership,
    };
    use pinion_core::scene::{ContainerNode, ExternalNode, Rect, Scene};

    const BOARD: DropContract = DropContract::new(
        const {
            &[DropClause::parts(
                "board-widget",
                DropActions::one(DropAction::Copy).with(DropAction::Move),
                const { &["slot-a", "slot-b"] },
            )]
        },
    );

    #[derive(Debug)]
    struct Target(DropContract);

    impl External for Target {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
            Some(self)
        }
        fn drop_offered(&mut self, _offer: &DropOffer) -> DropVerdict {
            DropVerdict::accept(DropAction::Copy, IntrospectValue::Null)
        }
    }

    impl ExternalIntrospect for Target {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn drop_contract(&self) -> DropContract {
            self.0
        }
        fn query(&self, _path: &str) -> Result<IntrospectValue, ReadRefusal> {
            Err(ReadRefusal::UnknownPath)
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            _method: &str,
            _args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            Err(InvokeError::UnknownPath)
        }
    }

    fn tagged(tag: &str, rect: Rect) -> Scene {
        let mut node = Scene::Container(ContainerNode::new(vec![]).with_tag(tag.to_string()));
        if let Scene::Container(c) = &mut node {
            c.rect = rect;
        }
        node
    }

    /// `board` 0..200 with two declared slots and one undeclared rim;
    /// `palette` 200..300 declares nothing.
    fn scenes() -> (Scene, Scene) {
        let mut board = Scene::Container(
            ContainerNode::new(vec![
                tagged("board#slot-a", Rect::new(0, 0, 200, 100)),
                tagged("board#slot-b", Rect::new(0, 100, 200, 100)),
                tagged("board#rim", Rect::new(0, 200, 200, 100)),
            ])
            .with_tag("board"),
        );
        if let Scene::Container(c) = &mut board {
            c.rect = Rect::new(0, 0, 200, 300);
        }
        let mut root = Scene::Container(ContainerNode::new(vec![
            board,
            tagged("palette", Rect::new(200, 0, 100, 300)),
        ]));
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, 300, 300);
        }
        let state = Scene::Container(ContainerNode::new(vec![
            Scene::External(ExternalNode::new(Box::new(Target(BOARD))).with_tag("board")),
            Scene::External(
                ExternalNode::new(Box::new(Target(DropContract::EMPTY))).with_tag("palette"),
            ),
        ]));
        (root, state)
    }

    fn call(kind: Option<&str>, action: Option<&str>, at: Option<(f64, f64)>) -> DropTargetsReport {
        let (paint, state) = scenes();
        let value = handle_scene_drop_targets(Some(&paint), &state, kind, action, at)
            .expect("the census serialises");
        serde_json::from_value(value).expect("the report round-trips")
    }

    #[test]
    fn r1734_the_census_publishes_the_declaration_without_judging_it() {
        let report = call(None, None, None);
        assert_eq!(report.declared, 1, "one of the two surfaces declares");
        assert_eq!(report.admitting, None, "nothing was judged");
        assert_eq!(report.kind, None);
        let board = report
            .surfaces
            .iter()
            .find(|s| s.surface == "board")
            .expect("the board is painted");
        assert_eq!(board.clauses.len(), 1);
        assert_eq!(board.clauses[0].kind, "board-widget");
        assert_eq!(board.clauses[0].actions, ["copy", "move"]);
        assert_eq!(board.clauses[0].parts, ["slot-a", "slot-b"]);
        assert_eq!(board.clauses[0].region, "parts");
        assert!(board.verdict.is_none(), "no kind named, no verdict");
        let palette = report
            .surfaces
            .iter()
            .find(|s| s.surface == "palette")
            .expect("the palette is painted");
        assert!(
            palette.clauses.is_empty(),
            "and it says so with an empty list"
        );
    }

    #[test]
    fn r1734_naming_a_kind_judges_every_surface_against_its_declaration() {
        let report = call(Some("board-widget"), None, None);
        assert_eq!(report.kind.as_deref(), Some("board-widget"));
        assert_eq!(
            report.actions,
            ["copy", "move", "link"],
            "no action named offers all three"
        );
        assert_eq!(report.admitting, Some(1));
        let board = report
            .surfaces
            .iter()
            .find(|s| s.surface == "board")
            .unwrap();
        let verdict = board.verdict.as_ref().expect("judged");
        assert!(verdict.admits);
        assert_eq!(
            verdict.actions,
            ["copy", "move"],
            "narrowed to the common set"
        );
        assert_eq!(verdict.refused, None);
        // The undeclaring surface is refused with the reason, not silently
        // omitted — a client asking "where can this go" needs the negatives too.
        let palette = report
            .surfaces
            .iter()
            .find(|s| s.surface == "palette")
            .unwrap();
        let verdict = palette.verdict.as_ref().expect("judged");
        assert!(!verdict.admits);
        assert_eq!(verdict.refused.as_deref(), Some("kind-not-accepted"));
        assert_eq!(
            verdict.why.as_deref(),
            Some("nothing can be dropped here, and this is a board-widget"),
        );
    }

    #[test]
    fn r1734_an_action_the_surface_cannot_do_is_refused_with_both_sides_named() {
        let report = call(Some("board-widget"), Some("link"), None);
        assert_eq!(report.actions, ["link"]);
        assert_eq!(report.admitting, Some(0));
        let board = report
            .surfaces
            .iter()
            .find(|s| s.surface == "board")
            .unwrap();
        let verdict = board.verdict.as_ref().unwrap();
        assert_eq!(verdict.refused.as_deref(), Some("no-common-action"));
        let why = verdict.why.as_deref().unwrap();
        assert!(why.contains("copy and move"), "{why}");
        assert!(why.contains("link"), "{why}");
    }

    #[test]
    fn r1734_a_point_names_the_part_a_release_there_would_resolve_to() {
        // Over slot-b: admitted, and the part is the one the ROUTER's own
        // resolver returns.
        let report = call(Some("board-widget"), Some("copy"), Some((100.0, 150.0)));
        let at = report.at.expect("a point was asked");
        assert_eq!(at.surface.as_deref(), Some("board"));
        assert_eq!(at.part.as_deref(), Some("slot-b"));
        assert!(at.verdict.as_ref().unwrap().admits);
        // Over the undeclared rim of the SAME surface: refused, and the
        // refusal names the parts that would have worked. A per-part answer no
        // boolean-per-widget interface can give.
        let report = call(Some("board-widget"), Some("copy"), Some((100.0, 250.0)));
        let at = report.at.expect("a point was asked");
        assert_eq!(at.part.as_deref(), Some("rim"));
        let verdict = at.verdict.as_ref().unwrap();
        assert!(!verdict.admits);
        assert_eq!(verdict.refused.as_deref(), Some("part-not-accepted"));
        let why = verdict.why.as_deref().unwrap();
        assert!(
            why.contains("slot-a, slot-b") || why.contains("slot-a and slot-b"),
            "{why}"
        );
    }

    #[test]
    fn r1734_an_unknown_action_word_is_refused_rather_than_guessed() {
        let (paint, state) = scenes();
        let err = handle_scene_drop_targets(
            Some(&paint),
            &state,
            Some("board-widget"),
            Some("teleport"),
            None,
        )
        .expect_err("teleport is not a drop action");
        assert!(format!("{err:?}").contains("UnknownAction"), "{err:?}");
    }

    #[test]
    fn r1734_a_screen_that_has_not_painted_answers_an_empty_census() {
        let (_, state) = scenes();
        let value = handle_scene_drop_targets(None, &state, Some("board-widget"), None, None)
            .expect("serialises");
        let report: DropTargetsReport = serde_json::from_value(value).expect("round-trips");
        assert!(report.surfaces.is_empty());
        assert_eq!(report.declared, 0);
        assert_eq!(report.admitting, Some(0), "judged nothing, which is zero");
    }
}
