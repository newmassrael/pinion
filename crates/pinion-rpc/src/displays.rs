//! `scene/displays` RPC method dispatch — R1576 §5.16 §5.41 §5.7 §5.12 §2 #7.
//!
//! The **desk** an application is running on: which monitors are attached,
//! where they sit relative to one another, and — the half that makes this more
//! than a list — where a given window rectangle would actually *land* on them.
//!
//! ## Why this method exists at all
//!
//! The toolkit has no peer at any price. `screens()` is in-process C++: nothing
//! outside a running the toolkit application can ask it what it is displaying
//! on. For pinion that is not a nicety — §2 #2 makes the RPC plane the agent's
//! primary path and §2 #7 says the scene is queryable as data — and a headless
//! agent that opens a second window, tears off a panel, or restores a layout
//! preset is placing pixels in a coordinate space it otherwise cannot see.
//!
//! Even *inside* the process, three of the questions here have no the toolkit
//! answer:
//!
//! * **Does the virtual desktop have holes?** `virtualGeometry()` is
//!   the bounding rectangle. On any arrangement that is not itself a rectangle
//!   it contains points that are on no screen, and the toolkit exposes no way to learn
//!   that — which is why the toolkit code so often uses `virtualGeometry()` containment
//!   as a visibility test and is wrong on every L-shaped desk. `gap_free` is
//!   that fact, and `covered_px` is the evidence for it.
//! * **Where would this rectangle be?** Every the toolkit screen query takes a *point*
//!   (`screenAt`, `virtualSiblingAt`). `placement` answers for a rectangle:
//!   which display holds the largest share, every display it touches, how many
//!   of its pixels are on a display at all, and the nearest origin that would
//!   make it wholly visible.
//! * **What happened to my saved layout?** `saveGeometry()` is an
//!   opaque byte array of absolute geometry, and `restoreGeometry` has
//!   nowhere to report that it put the window somewhere else. `anchored`
//!   resolves a display-relative anchor and **names** the substitution when the
//!   display it asks for is gone.
//!
//! ## Shape
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/displays", "id": 1 }
//! ```
//!
//! ```json
//! {
//!   "displays": [
//!     { "id": "dp-4", "label": "DP-4", "primary": true, "scale": 1.0,
//!       "refresh_mhz": 59940, "bounds": {"x": 0, "y": 0, "w": 2560, "h": 1600},
//!       "logical_size": {"w": 2560.0, "h": 1600.0} }
//!   ],
//!   "primary": "dp-4",
//!   "fallback": "dp-4",
//!   "bounding_box": {"x": 0, "y": 0, "w": 2560, "h": 1600},
//!   "covered_px": 4096000,
//!   "gap_free": true
//! }
//! ```
//!
//! An **empty** `displays` list is a real state — a headless or surfaceless session —
//! not an error, so this method has no `*Unavailable` token. The toolkit models the same
//! state as `primaryScreen()` answering `nullptr`, which is the shape that produces the crash
//! rather than the answer.
//!
//! Three optional parameters add a derived answer each, and each is **absent**
//! from the response unless it was asked for:
//!
//! * `at: {x, y}` — physical point → `at`, the display containing it, or
//!   `null`. The toolkit `screenAt`.
//! * `probe: {x, y, w, h}` — physical rectangle → `placement`.
//! * `anchor: {display, offset: [x, y]}` — a preset's place → `anchored`.
//!
//! ## Side-effect contract
//!
//! Read-only. The embedder reads the platform's monitor list and hands it in;
//! answering moves no window, changes no declaration and schedules no repaint.

use pinion_core::display::{Anchor, Anchored, Display, DisplayId, DisplayRect, DisplayTopology};
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// A rectangle in the virtual desktop, physical device pixels, signed origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DisplayRectOutcome {
    /// Left edge. Negative for a display left of the primary one.
    pub x: i32,
    /// Top edge. Negative for a display above the primary one.
    pub y: i32,
    /// Width in physical pixels.
    pub w: u32,
    /// Height in physical pixels.
    pub h: u32,
}

impl From<DisplayRect> for DisplayRectOutcome {
    fn from(r: DisplayRect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        }
    }
}

/// A display's size once its own scale factor is divided out.
///
/// Published rather than left to the client because the division is per
/// display, and a client that does it against the wrong display's scale gets a
/// plausible wrong number — the exact mistake the physical coordinate space
/// exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LogicalSizeOutcome {
    /// Logical width.
    pub w: f64,
    /// Logical height.
    pub h: f64,
}

/// One monitor on the wire.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisplayOutcome {
    /// The address a layout preset names — unique within this response by
    /// construction. The toolkit has no accessor for this at all: `name()` is
    /// platform text with no uniqueness guarantee, so the only handle the
    /// toolkit offers is a `screen *` that dies on `screenRemoved`.
    pub id: String,
    /// The platform's own name, verbatim. May be empty, and may repeat across
    /// displays — which is precisely why it is not the id.
    pub label: String,
    /// Physical bounds in the virtual desktop.
    pub bounds: DisplayRectOutcome,
    /// Physical pixels per logical pixel. The toolkit `devicePixelRatio`.
    pub scale: f64,
    /// This display's logical extent, derived from `bounds` and `scale`.
    pub logical_size: LogicalSizeOutcome,
    /// Refresh rate in millihertz, or `null` when the platform did not report
    /// one. `refreshRate()` answers `qreal`, so an unknown rate
    /// arrives there as a real-looking `0`.
    pub refresh_mhz: Option<u32>,
    /// Is this the primary display? At most one display in a response is.
    pub primary: bool,
}

impl From<&Display> for DisplayOutcome {
    fn from(d: &Display) -> Self {
        let (lw, lh) = d.logical_size();
        Self {
            id: d.id().as_str().to_owned(),
            label: d.label().to_owned(),
            bounds: d.bounds().into(),
            scale: d.scale_factor(),
            logical_size: LogicalSizeOutcome { w: lw, h: lh },
            refresh_mhz: d.refresh_mhz(),
            primary: d.primary(),
        }
    }
}

/// One display's share of a probed rectangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoverageOutcome {
    /// Which display.
    pub id: String,
    /// How many of the rectangle's pixels land on it. Counted **per display**,
    /// so these sum to more than `visible_px` wherever two displays overlap (a
    /// mirrored pair). Both numbers are published so that relation is
    /// checkable rather than a thing a client has to trust.
    pub px: u64,
}

/// Where a probed rectangle actually is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlacementOutcome {
    /// The display holding the largest share, or `null` when the rectangle is
    /// on no display at all — the state a restored preset lands in after a
    /// monitor is unplugged.
    pub home: Option<String>,
    /// Every display the rectangle touches, in enumeration order.
    pub covering: Vec<CoverageOutcome>,
    /// Pixels of the rectangle on some display, counting overlap once.
    pub visible_px: u64,
    /// The rectangle's own pixel count.
    pub total_px: u64,
    /// Pixels of the rectangle on no display. `total_px - visible_px`,
    /// published because "how much of my window is lost" is the question, and
    /// making the client subtract invites it to subtract the wrong pair.
    pub offscreen_px: u64,
    /// Is every pixel of it on some display? `false` for an empty rectangle,
    /// which has no pixels anywhere.
    pub fully_visible: bool,
    /// The visible share, `0.0` for an empty rectangle.
    pub visible_fraction: f64,
    /// The nearest origin that would put the rectangle wholly inside a single
    /// display, or `null` when no display is big enough. Present even when it
    /// is already wholly visible, where it equals the rectangle's own origin.
    pub suggestion: Option<[i32; 2]>,
}

impl From<pinion_core::display::Placement> for PlacementOutcome {
    fn from(p: pinion_core::display::Placement) -> Self {
        Self {
            home: p.home.as_ref().map(|id| id.as_str().to_owned()),
            covering: p
                .covering
                .iter()
                .map(|c| CoverageOutcome {
                    id: c.id.as_str().to_owned(),
                    px: c.px,
                })
                .collect(),
            visible_px: p.visible_px,
            total_px: p.total_px,
            offscreen_px: p.offscreen_px(),
            fully_visible: p.is_fully_visible(),
            visible_fraction: p.visible_fraction(),
            suggestion: p.suggestion.map(|(x, y)| [x, y]),
        }
    }
}

/// What became of a display-relative anchor against today's desk.
///
/// Flattened rather than a tagged union, because the three outcomes share two
/// of their three fields and a client branches on `kind` either way. `kind` is
/// a closed vocabulary this crate owns — `on_declared` / `substituted` /
/// `no_display` — so a client may match on it (R1565's `data_is_prose`
/// distinction).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnchoredOutcome {
    /// `on_declared`, `substituted`, or `no_display`.
    pub kind: &'static str,
    /// The display the anchor asked for. Always present — an anchor that was
    /// honoured still says what it asked for, so one field answers "what did I
    /// request" across all three outcomes.
    pub declared: String,
    /// The display actually used, or `null` on a headless desk.
    pub display: Option<String>,
    /// The absolute physical origin, or `null` on a headless desk.
    pub at: Option<[i32; 2]>,
}

impl From<(&Anchor, Anchored)> for AnchoredOutcome {
    fn from((asked, got): (&Anchor, Anchored)) -> Self {
        Self {
            kind: got.name(),
            declared: asked.display.as_str().to_owned(),
            display: got.display().map(|id| id.as_str().to_owned()),
            at: got.at().map(|(x, y)| [x, y]),
        }
    }
}

/// The answer to the optional `at` question: which display holds a point.
///
/// An object rather than a bare nullable string so "asked, and the point is on
/// no display" (`{"display": null}`) stays distinct from "did not ask" (the key
/// absent). A single nullable field cannot carry both, and collapsing them
/// would make an agent probing a coordinate unable to tell a real answer from
/// its own forgotten parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayAtOutcome {
    /// The display containing the point, or `null` when it is on none. The
    /// toolkit `screenAt` answers `nullptr` for the same case.
    pub display: Option<String>,
}

/// The `scene/displays` response body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisplaysOutcome {
    /// Every attached monitor, in the platform's enumeration order. Empty on a
    /// headless session, which is a state rather than a failure.
    pub displays: Vec<DisplayOutcome>,
    /// The primary display's id, or `null` when the platform reports none.
    pub primary: Option<String>,
    /// The display a vanished anchor substitutes onto: the primary if there is
    /// one, else the first enumerated. Distinct from `primary` because a
    /// platform reporting no primary at all still has to answer "where does a
    /// homeless window go".
    pub fallback: Option<String>,
    /// The smallest rectangle containing every display, or `null` when there
    /// are none. The toolkit `virtualGeometry`.
    pub bounding_box: Option<DisplayRectOutcome>,
    /// Pixels on at least one display, counting an overlap once.
    pub covered_px: u64,
    /// Does the arrangement fill its own bounding box? `false` means there are
    /// points inside `bounding_box` that are on no display — the fact the toolkit has no
    /// accessor for.
    pub gap_free: bool,
    /// The display containing the requested `at` point. Absent unless `at` was
    /// asked for; see [`DisplayAtOutcome`] for why it is an object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<DisplayAtOutcome>,
    /// Where the requested `probe` rectangle would be. Absent unless asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementOutcome>,
    /// What became of the requested `anchor`. Absent unless asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchored: Option<AnchoredOutcome>,
}

/// R1576 §5.16 §5.12 — project a topology, plus whatever derived answers the
/// request asked for, onto the wire.
///
/// The three optional questions are answered here rather than by a client
/// re-implementing the geometry, which is the point: `pinion-core` owns one
/// implementation, the shell places windows with it, and the wire reports it —
/// so the answer and the behaviour cannot disagree. A client computing
/// `home` itself from `bounds` would be a second implementation, and the first
/// arrangement with a mirrored pair would separate them.
#[must_use]
pub fn displays(topology: &DisplayTopology, ask: &DisplayAsk) -> DisplaysOutcome {
    DisplaysOutcome {
        displays: topology.iter().map(DisplayOutcome::from).collect(),
        primary: topology.primary().map(|d| d.id().as_str().to_owned()),
        fallback: topology.fallback().map(|d| d.id().as_str().to_owned()),
        bounding_box: topology.bounding_box().map(DisplayRectOutcome::from),
        covered_px: topology.covered_px(),
        gap_free: topology.is_gap_free(),
        at: ask.at.map(|(x, y)| DisplayAtOutcome {
            display: topology
                .display_at(x, y)
                .map(|d| d.id().as_str().to_owned()),
        }),
        placement: ask.probe.map(|r| topology.resolve(r).into()),
        anchored: ask
            .anchor
            .as_ref()
            .map(|a| AnchoredOutcome::from((a, topology.anchor(a)))),
    }
}

/// The optional derived answers a `scene/displays` request asks for.
///
/// A separate value rather than three parameters, so a caller that asks for
/// nothing spells that as [`DisplayAsk::none`] and the growth path for a fourth
/// question does not re-sign every call site.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayAsk {
    /// A physical point to locate.
    pub at: Option<(i32, i32)>,
    /// A physical rectangle to resolve.
    pub probe: Option<DisplayRect>,
    /// A display-relative anchor to resolve.
    pub anchor: Option<Anchor>,
}

impl DisplayAsk {
    /// Ask for the topology alone.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Parse the three optional parameters out of a request.
    ///
    /// **Loud on malformed input.** A `probe` missing its `w` is a client bug,
    /// and answering it with the bare topology would look like a successful
    /// call whose placement key the client then reads as absent-because-off-
    /// screen. Each refusal **names the offending parameter path**, in the
    /// `Word: detail` shape `UnknownWindow` already uses — `-32602` publishes a
    /// closed `data` vocabulary (R1565), so the payload stays matchable by
    /// prefix and the path after the colon says which key to fix. What that key
    /// must contain is this module's own doc, not a sentence on the wire.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError::invalid_params`] when a parameter is present but not
    /// the shape this method needs.
    pub fn parse(params: Option<&Value>) -> Result<Self, RpcError> {
        let Some(obj) = params.and_then(Value::as_object) else {
            return Ok(Self::none());
        };
        let mut ask = Self::none();
        if let Some(at) = obj.get("at") {
            let (x, y) = point(at, "at")?;
            ask.at = Some((x, y));
        }
        if let Some(probe) = obj.get("probe") {
            let o = probe.as_object().ok_or_else(|| invalid("probe"))?;
            let signed = |k: &str| -> Result<i32, RpcError> {
                o.get(k)
                    .and_then(Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| invalid(&format!("probe.{k}")))
            };
            let extent = |k: &str| -> Result<u32, RpcError> {
                o.get(k)
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| invalid(&format!("probe.{k}")))
            };
            ask.probe = Some(DisplayRect::new(
                signed("x")?,
                signed("y")?,
                extent("w")?,
                extent("h")?,
            ));
        }
        if let Some(anchor) = obj.get("anchor") {
            let o = anchor.as_object().ok_or_else(|| invalid("anchor"))?;
            let display = o
                .get("display")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("anchor.display"))?;
            let offset = o.get("offset").ok_or_else(|| invalid("anchor.offset"))?;
            let (x, y) = point(offset, "anchor.offset")?;
            ask.anchor = Some(Anchor::new(DisplayId::new(display), (x, y)));
        }
        Ok(ask)
    }
}

/// A `[x, y]` array or an `{x, y}` object, both spellings accepted because the
/// rest of this wire uses both and a caller should not have to remember which.
fn point(value: &Value, name: &str) -> Result<(i32, i32), RpcError> {
    let pair = match value {
        Value::Array(a) if a.len() == 2 => (a[0].as_i64(), a[1].as_i64()),
        Value::Object(o) => (
            o.get("x").and_then(Value::as_i64),
            o.get("y").and_then(Value::as_i64),
        ),
        _ => (None, None),
    };
    match pair {
        (Some(x), Some(y)) => Ok((
            i32::try_from(x).map_err(|_| invalid(name))?,
            i32::try_from(y).map_err(|_| invalid(name))?,
        )),
        _ => Err(invalid(name)),
    }
}

/// The one refusal this method makes: the word, then the offending parameter
/// path.
///
/// It carries the path and not a sentence because `-32602`'s `data` is a closed
/// vocabulary a client may MATCH (R1565's `data_is_prose` rule), and free prose
/// there would break the one guarantee that payload carries. What each
/// parameter must contain is this module's own doc — a fact about the method,
/// not about this call.
fn invalid(name: &str) -> RpcError {
    RpcError::invalid_params(String::new()).with_data_string(format!("MalformedDisplayAsk: {name}"))
}

#[cfg(test)]
mod tests {
    use super::{DisplayAsk, DisplayRect, DisplaysOutcome, displays};
    use pinion_core::display::{DisplayInfo, DisplayTopology};
    use serde_json::json;

    fn two_panels() -> DisplayTopology {
        DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", DisplayRect::new(0, 0, 1920, 1080))
                .as_primary()
                .with_refresh_mhz(59_940),
            DisplayInfo::new("DP-2", DisplayRect::new(1920, 0, 1920, 1080)),
        ])
    }

    fn out(ask: &DisplayAsk) -> DisplaysOutcome {
        displays(&two_panels(), ask)
    }

    #[test]
    fn r1576_a_headless_desk_is_a_state_not_an_error() {
        let o = displays(&DisplayTopology::empty(), &DisplayAsk::none());
        assert!(o.displays.is_empty());
        assert_eq!(o.primary, None);
        assert_eq!(o.fallback, None);
        assert_eq!(o.bounding_box, None);
        assert_eq!(o.covered_px, 0);
        assert!(o.gap_free, "an empty union fills an empty bounding box");
    }

    #[test]
    fn r1576_the_topology_is_published_with_its_derived_facts() {
        let o = out(&DisplayAsk::none());
        assert_eq!(o.displays.len(), 2);
        assert_eq!(o.primary.as_deref(), Some("dp-1"));
        assert_eq!(o.fallback.as_deref(), Some("dp-1"));
        assert_eq!(o.covered_px, 3840 * 1080);
        assert!(o.gap_free);
        assert_eq!(o.displays[0].refresh_mhz, Some(59_940));
        assert_eq!(
            o.displays[1].refresh_mhz, None,
            "an unreported rate is null, never a plausible 0"
        );
        assert_eq!(o.displays[0].label, "DP-1");
        assert_eq!(o.displays[0].id, "dp-1", "the id is derived, not the label");
    }

    #[test]
    fn r1576_the_three_derived_answers_are_absent_unless_asked_for() {
        let json = serde_json::to_value(out(&DisplayAsk::none())).expect("serializes");
        let obj = json.as_object().expect("object");
        for key in ["at", "placement", "anchored"] {
            assert!(
                !obj.contains_key(key),
                "{key} must be absent, not null: a null reads as an answer"
            );
        }
        assert!(obj.contains_key("gap_free"), "but the desk itself is there");
    }

    #[test]
    fn r1576_a_probe_reports_the_lost_pixels_and_the_way_back() {
        let ask = DisplayAsk {
            probe: Some(DisplayRect::new(3740, 0, 200, 100)),
            ..DisplayAsk::none()
        };
        let p = out(&ask).placement.expect("asked for");
        assert_eq!(p.home.as_deref(), Some("dp-2"));
        assert_eq!(p.visible_px, 10_000);
        assert_eq!(p.total_px, 20_000);
        assert_eq!(p.offscreen_px, 10_000);
        assert!(!p.fully_visible);
        assert_eq!(p.suggestion, Some([3640, 0]));
        // The published sum is consistent with itself — the analyzer-class
        // discipline of putting the total check IN the probe.
        assert_eq!(p.visible_px + p.offscreen_px, p.total_px);
    }

    #[test]
    fn r1576_a_point_may_be_spelled_either_way() {
        for at in [json!([2000, 10]), json!({"x": 2000, "y": 10})] {
            let ask = DisplayAsk::parse(Some(&json!({ "at": at }))).expect("parses");
            assert_eq!(
                out(&ask).at.expect("asked for").display.as_deref(),
                Some("dp-2")
            );
        }
        // A point on no display is an answer of `null` INSIDE a present key —
        // distinct from the key being absent because nothing was asked.
        let ask = DisplayAsk::parse(Some(&json!({"at": [9999, 9999]}))).expect("parses");
        assert_eq!(out(&ask).at.expect("asked for").display, None);
    }

    #[test]
    fn r1576_an_anchor_names_what_it_asked_for_in_every_outcome() {
        let ask = DisplayAsk::parse(Some(&json!({
            "anchor": {"display": "dp-2", "offset": [40, 30]}
        })))
        .expect("parses");
        let a = out(&ask).anchored.expect("asked for");
        assert_eq!(a.kind, "on_declared");
        assert_eq!(a.declared, "dp-2");
        assert_eq!(a.display.as_deref(), Some("dp-2"));
        assert_eq!(a.at, Some([1960, 30]));

        // The same anchor against a desk that lost that monitor.
        let one = DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", DisplayRect::new(0, 0, 1920, 1080)).as_primary(),
        ]);
        let a = displays(&one, &ask).anchored.expect("asked for");
        assert_eq!(a.kind, "substituted");
        assert_eq!(a.declared, "dp-2", "what the preset asked for survives");
        assert_eq!(a.display.as_deref(), Some("dp-1"));
        assert_eq!(a.at, Some([40, 30]));
    }

    #[test]
    fn r1576_a_malformed_ask_is_refused_and_names_the_parameter() {
        let cases = [
            (json!({"probe": {"x": 0, "y": 0, "w": 10}}), "probe.h"),
            (json!({"probe": 7}), "probe"),
            (json!({"at": [1]}), "at"),
            (json!({"anchor": {"offset": [0, 0]}}), "anchor.display"),
            (json!({"anchor": {"display": "dp-1"}}), "anchor.offset"),
        ];
        for (params, named) in cases {
            let err = DisplayAsk::parse(Some(&params)).expect_err("refused");
            let text = format!("{err:?}");
            assert!(
                text.contains(named),
                "the refusal must name `{named}`, got {text}"
            );
        }
        // No parameters at all is not malformed — it is the common call.
        assert_eq!(DisplayAsk::parse(None).expect("ok"), DisplayAsk::none());
        assert_eq!(
            DisplayAsk::parse(Some(&json!({}))).expect("ok"),
            DisplayAsk::none()
        );
    }
}
