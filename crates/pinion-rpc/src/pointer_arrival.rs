//! ★★★★★ R1737 §5.35 §5.15 §5.12 §2 #7 — `scene/pointer_arrival`: **where a
//! pointer arrived in every surface, and whether the framework's two accounts
//! of it agree.**
//!
//! # The question this answers, and who could not ask it before
//!
//! A pinion screen is one [`External`](pinion_core::external::External) (§2 #7),
//! so the router hands it a *fraction* of its painted rectangle and the screen
//! multiplies that back to a pixel. Every press on such a screen is resolved
//! against that pixel — a press carries no position of its own, it acts on the
//! cursor the last `pointer_move` recorded — so a wrong pixel is a wrong aim for
//! every gesture on the screen.
//!
//! R1736 found exactly that: a fraction built by an `f32` division, multiplied
//! back and truncated, landing on the pixel *before* the one the pointer was
//! over, at some coordinates and not others. It was found by walking a real X
//! pointer over 600 columns and 600 rows of a running screen and asking the
//! screen where it thought the pointer was — **which only works on a screen that
//! publishes a cursor.**
//!
//! Measured across the five screens in this tree that hit-test themselves:
//! three publish a cursor field, in **two incompatible spellings** (a `"x,y"`
//! string and an `{x, y}` object), and **two publish nothing at all**. So the
//! measurement that found the defect was not runnable on two of the five
//! screens, and on the other three it depended on each screen's own vocabulary.
//!
//! The fact was never the screen's. The framework resolves the reading, and at
//! that moment it holds both accounts of where the pointer is: the cursor the
//! window system reported, and the rectangle the fraction is taken over. This
//! method publishes the comparison — see
//! [`pinion_core::arrival::Landing`] — for **every** surface, in one
//! spelling, whether or not the surface says anything about cursors.
//!
//! # Why a verdict and not two numbers
//!
//! Because holding both and comparing neither is the failure mode this tree
//! keeps measuring in the floor. Built as a probe against 6.11.1 and run: an
//! item whose paint reaches six pixels past its pick shape leaves 15.4% of its
//! painted pixels dead at zoom 100, while the framework holds both rectangles
//! and never compares them (R1736). The same shape appears here — that
//! framework can compute a widget-local pointer position for any widget without
//! the widget storing anything, exactly, over 400 columns and 300 rows, so
//! universality is the floor's and this tree owed it; but its answer is **where
//! the cursor is now**, not where the event *arrived*. Measured: a press
//! delivered to a child at (37, 21), then the cursor moved, leaves it answering
//! (300, 250). Across the five types such a record could live on there are 245
//! declared properties and 195 declared methods, of which 3 are point-typed
//! (all the widget's own position in its parent) and none is a delivered
//! position.
//!
//! So the floor can tell you where the mouse is. It cannot tell you where the
//! event that moved your widget landed, and it cannot tell you that the two
//! disagree.
//!
//! # The three answers, and why "never" is one of them
//!
//! * `exact` — the pointer was inside the rectangle and the pixel the surface
//!   resolves is the pixel it was over.
//! * `drifted` — both name a pixel of the rectangle and they are different
//!   pixels. **A defect**, and the only arm that is.
//! * `strayed` — the cursor was outside the rectangle the fraction was taken
//!   over, which a capture lock does on purpose, so the resolved pixel is a
//!   clamp rather than a claim.
//! * `never` — no pointer has reached this surface. Reported as its own state
//!   rather than folded into a clean total, the same discipline
//!   [`PointerTarget::Unanswered`](pinion_core::external::PointerTarget::Unanswered)
//!   applies: a surface nobody has pointed at must not read as a surface that
//!   checked out.
//!
//! # Example
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/pointer_arrival", "id": 1 }
//! ```
//!
//! ```json
//! {
//!   "surfaces": [
//!     {
//!       "surface": "node_lab",
//!       "state": "arrived",
//!       "delivered": 600,
//!       "exact": 600,
//!       "strayed": 0,
//!       "drifted": 0,
//!       "last": {
//!         "over": { "x": 0, "y": 0, "w": 1625, "h": 900 },
//!         "cursor": [434.0, 143.0],
//!         "inside": [434, 143],
//!         "resolved": [434, 143],
//!         "drift": [0, 0],
//!         "landing": "exact"
//!       }
//!     }
//!   ],
//!   "never": [],
//!   "arrived": 1,
//!   "delivered": 600,
//!   "defects": 0,
//!   "drifts": 0
//! }
//! ```
//!
//! # Why the counts are here and not only the last arrival
//!
//! Because a check over the last arrival is a check over one event, and which
//! event that is is an accident of when the caller asked. That is R1736's own
//! finding one level up: `scene/pointer_target` probed nine points and only the
//! middle could convict, so the gate's coverage was decided by which point it
//! happened to look at.
//!
//! With the framework counting, a caller may drive six hundred pointer
//! positions and ask **once**, and the answer is about all six hundred —
//! `drifted_at` keeps the first bad one as the evidence. It is also what makes
//! the check affordable: a round trip per pixel loaded this machine enough to
//! time out the boot of the next screen the sweep was about to measure.

use pinion_core::Scene;
use pinion_core::arrival::{Landing, PointerArrival, SurfaceArrivals, pointer_arrival};
use pinion_core::scene::Rect;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;
use crate::resolve::painted_surfaces;

/// A rectangle on the wire.
///
/// Spelled out rather than serialising [`Rect`] directly because `Rect` is
/// `#[non_exhaustive]` and carries no `Serialize`: a wire shape is a promise,
/// and deriving it from a type that may grow a field would change the promise
/// without anybody deciding to.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct WireRect {
    /// Left edge, in the window's frame.
    pub x: u32,
    /// Top edge, in the window's frame.
    pub y: u32,
    /// Width in logical pixels.
    pub w: u32,
    /// Height in logical pixels.
    pub h: u32,
}

impl From<Rect> for WireRect {
    fn from(rect: Rect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
        }
    }
}

/// One arrival, spelled out — both accounts and the verdict between them.
///
/// Used twice in a row: for the surface's most recent arrival, and for the
/// **first** one that went wrong. The same shape for both, because a reader
/// comparing "where it is now" with "where it went wrong" should not have to
/// learn two vocabularies for one fact.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ArrivalAt {
    /// The rectangle the fraction was taken over, as the paint laid it out.
    pub over: WireRect,
    /// The cursor the window system reported, in the window's frame.
    pub cursor: (f64, f64),
    /// The pixel of `over` the window system put the pointer at.
    pub inside: (i64, i64),
    /// The pixel the surface resolves the delivered fraction to.
    pub resolved: (u32, u32),
    /// `resolved − inside`. Zero on an exact landing, and zero on a strayed one
    /// because the resolved pixel there is a clamp whose difference means
    /// nothing — published either way so a caller reads one number rather than
    /// subtracting two whose subtraction may be meaningless.
    pub drift: (i32, i32),
    /// `"exact"`, `"drifted"` or `"strayed"`.
    pub landing: &'static str,
}

impl From<PointerArrival> for ArrivalAt {
    fn from(arrival: PointerArrival) -> Self {
        let landing = arrival.landing();
        Self {
            over: arrival.over.into(),
            cursor: arrival.cursor,
            inside: arrival.inside(),
            resolved: arrival.resolved(),
            drift: match landing {
                Landing::Drifted { by } => by,
                Landing::Exact | Landing::Strayed => (0, 0),
            },
            landing: landing.word(),
        }
    }
}

/// One surface's arrival row.
#[derive(Debug, Clone, Serialize)]
pub struct ArrivalRow {
    /// The surface's tag.
    pub surface: String,
    /// `"arrived"` or `"never"`.
    pub state: &'static str,
    /// How many arrivals this surface has been delivered. Zero is not spelled:
    /// a `never` row omits every count, so an unexercised surface cannot be read
    /// as an exercised one with nothing wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered: Option<u64>,
    /// How many landed on the pixel the pointer was over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<u64>,
    /// How many arrived with the cursor outside the rectangle (a capture lock).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strayed: Option<u64>,
    /// How many named a different pixel of the rectangle. Any is a defect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drifted: Option<u64>,
    /// The most recent arrival — where this surface thinks the pointer is now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<ArrivalAt>,
    /// The FIRST arrival that went wrong, kept as the evidence. Absent when none
    /// did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drifted_at: Option<ArrivalAt>,
}

impl ArrivalRow {
    /// The row for a surface a pointer has reached.
    fn arrived(surface: String, tally: &SurfaceArrivals) -> Self {
        Self {
            surface,
            state: "arrived",
            delivered: Some(tally.delivered),
            exact: Some(tally.exact),
            strayed: Some(tally.strayed),
            drifted: Some(tally.drifted),
            last: Some(tally.last.into()),
            drifted_at: tally.drifted_at.map(Into::into),
        }
    }

    /// The row for a surface no pointer has reached.
    fn never(surface: String) -> Self {
        Self {
            surface,
            state: "never",
            delivered: None,
            exact: None,
            strayed: None,
            drifted: None,
            last: None,
            drifted_at: None,
        }
    }
}

/// The `scene/pointer_arrival` result.
#[derive(Debug, Clone, Serialize)]
pub struct PointerArrivalReport {
    /// One row per painted `External`, in scene order.
    pub surfaces: Vec<ArrivalRow>,
    /// The surfaces no pointer has reached, named rather than counted as clean.
    pub never: Vec<String>,
    /// How many surfaces a pointer has reached.
    pub arrived: usize,
    /// How many arrivals, over all surfaces, were delivered. Published because
    /// "nothing drifted" is trivially true of a run that pointed at nothing, and
    /// this is the number that says whether it did.
    pub delivered: u64,
    /// How many surfaces had at least one arrival go wrong. A pure function of
    /// `surfaces`, published so the rule deciding "is this screen defective" has
    /// one version rather than one per consumer — the same reason
    /// [`crate::pointer_target::PointerTargetReport::defects`] exists.
    pub defects: usize,
    /// How many individual arrivals went wrong, over all surfaces.
    pub drifts: u64,
}

/// R1737 §5.35 — build the report from the painted scene and the state scene.
///
/// A screen that has not painted answers an empty report rather than an error,
/// for the reason [`crate::pointer_target`] does: "nothing disagrees" is true of
/// a screen that does not exist yet, and an error would make the check
/// impossible to run before the first frame.
///
/// # Errors
///
/// Only [`RpcError::internal_error`], when the report fails to serialise —
/// propagated rather than unwrapped because a panic in a dispatcher takes the
/// whole surface down.
pub fn handle_scene_pointer_arrival(
    last_paint_scene: Option<&Scene>,
    state_scene: &Scene,
) -> Result<Value, RpcError> {
    let report = last_paint_scene.map_or_else(
        || PointerArrivalReport {
            surfaces: Vec::new(),
            never: Vec::new(),
            arrived: 0,
            delivered: 0,
            defects: 0,
            drifts: 0,
        },
        |paint| build(paint, state_scene),
    );
    serde_json::to_value(&report).map_err(|e| RpcError::internal_error(e.to_string()))
}

/// The population is [`painted_surfaces`] and not the arrival store, and that
/// direction is load-bearing.
///
/// A report built by walking what the store happens to hold could only ever
/// list surfaces a pointer already reached — so a screen the pointer never got
/// to would be invisible rather than reported, and "no surface drifted" would be
/// satisfied by a run that pointed at nothing. Asking the SCENE which surfaces
/// exist is what makes `never` a row rather than a silence.
fn build(paint: &Scene, state_scene: &Scene) -> PointerArrivalReport {
    let mut surfaces = Vec::new();
    let mut never = Vec::new();
    let mut arrived = 0;
    let mut delivered = 0_u64;
    let mut defects = 0;
    let mut drifts = 0_u64;
    for (tag, _) in painted_surfaces(paint, state_scene) {
        if let Some(tally) = pointer_arrival(&tag) {
            arrived += 1;
            delivered = delivered.saturating_add(tally.delivered);
            drifts = drifts.saturating_add(tally.drifted);
            if tally.is_defective() {
                defects += 1;
            }
            surfaces.push(ArrivalRow::arrived(tag, &tally));
        } else {
            never.push(tag.clone());
            surfaces.push(ArrivalRow::never(tag));
        }
    }
    PointerArrivalReport {
        surfaces,
        never,
        arrived,
        delivered,
        defects,
        drifts,
    }
}

#[cfg(test)]
mod tests {
    use super::{ArrivalRow, handle_scene_pointer_arrival};
    use pinion_core::arrival::{
        PointerArrival, forget_pointer_arrival, pointer_arrival, record_pointer_arrival,
    };
    use pinion_core::scene::Rect;

    /// Record `arrivals` under `tag` and hand back the tally, so a wire test
    /// exercises the SAME store the census reads rather than a hand-built tally.
    fn tallied(tag: &str, arrivals: &[PointerArrival]) -> pinion_core::arrival::SurfaceArrivals {
        forget_pointer_arrival(tag);
        for arrival in arrivals {
            record_pointer_arrival(tag, *arrival);
        }
        let tally = pointer_arrival(tag).expect("just recorded");
        forget_pointer_arrival(tag);
        tally
    }

    fn exact(px: u32) -> PointerArrival {
        #[allow(clippy::cast_precision_loss)]
        PointerArrival::new(
            Rect::new(0, 0, 600, 400),
            (f64::from(px), 143.0),
            (px as f32 / 600.0, 143.0 / 400.0),
        )
    }

    fn one_short(px: u32) -> PointerArrival {
        #[allow(clippy::cast_precision_loss)]
        PointerArrival::new(
            Rect::new(0, 0, 600, 400),
            (f64::from(px), 143.0),
            ((px - 1) as f32 / 600.0, 143.0 / 400.0),
        )
    }

    /// The wire shape of a surface whose every arrival landed where it was
    /// aimed.
    #[test]
    fn r1737_an_exact_run_reports_its_counts_and_no_evidence() {
        let tally = tallied("r1737.rpc.exact", &[exact(434), exact(435), exact(436)]);
        let row = ArrivalRow::arrived("probe".to_owned(), &tally);
        assert_eq!(row.state, "arrived");
        assert_eq!(
            (row.delivered, row.exact, row.drifted),
            (Some(3), Some(3), Some(0))
        );
        let last = row
            .last
            .expect("a delivered surface reports its last arrival");
        assert_eq!(last.landing, "exact");
        assert_eq!(last.inside, (436, 143));
        assert_eq!(last.resolved, (436, 143));
        assert_eq!(last.drift, (0, 0));
        assert!(
            row.drifted_at.is_none(),
            "no evidence, because nothing went wrong"
        );
    }

    /// ★★★★★ And of a run with ONE bad arrival among many good ones — the shape
    /// a 600-pixel sweep produces and the shape a last-arrival-only check could
    /// not see. The evidence is the first drift, not the last thing that
    /// happened.
    #[test]
    fn r1737_one_drift_in_a_long_run_is_reported_with_its_own_evidence() {
        let mut run: Vec<PointerArrival> = (100..110).map(exact).collect();
        run.push(one_short(434));
        run.extend((200..205).map(exact));
        let tally = tallied("r1737.rpc.drift", &run);
        let row = ArrivalRow::arrived("probe".to_owned(), &tally);
        assert_eq!(
            (row.delivered, row.exact, row.drifted),
            (Some(16), Some(15), Some(1))
        );
        let last = row.last.expect("last");
        assert_eq!(last.landing, "exact", "the surface is fine RIGHT NOW");
        let evidence = row.drifted_at.expect("and it is still convicted");
        assert_eq!(evidence.landing, "drifted");
        assert_eq!(evidence.inside, (434, 143));
        assert_eq!(evidence.resolved, (433, 143));
        assert_eq!(evidence.drift, (-1, 0));
    }

    /// A surface no pointer reached carries no numbers at all, so a reader
    /// cannot mistake an absent arrival for one at the origin — nor an
    /// unexercised surface for an exercised one with nothing wrong.
    #[test]
    fn r1737_a_never_pointed_surface_carries_no_position_and_no_counts() {
        let row = ArrivalRow::never("probe".to_owned());
        assert_eq!(row.state, "never");
        assert!(row.last.is_none());
        assert!(row.delivered.is_none());
        let json = serde_json::to_value(&row).expect("a row serialises");
        for absent in [
            "last",
            "delivered",
            "exact",
            "drifted",
            "strayed",
            "drifted_at",
        ] {
            assert!(
                json.get(absent).is_none(),
                "an absent {absent} is absent from the wire, not null or zero: {json}"
            );
        }
    }

    /// Without a painted scene the report is empty rather than an error — the
    /// boot-time-runnable rule the sibling censuses follow.
    #[test]
    fn r1737_no_paint_yet_is_an_empty_report_not_a_failure() {
        let scene = pinion_core::Scene::Container(pinion_core::scene::ContainerNode::default());
        let value = handle_scene_pointer_arrival(None, &scene).expect("an empty report");
        assert_eq!(value["arrived"], 0);
        assert_eq!(value["delivered"], 0);
        assert_eq!(value["defects"], 0);
        assert_eq!(value["drifts"], 0);
        assert_eq!(value["surfaces"].as_array().map(Vec::len), Some(0));
    }
}
