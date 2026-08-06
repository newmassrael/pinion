// R838 §5.38 — example bindings tolerate looser doc-markdown lints than the
// substrate crates; the narrative carries many proper-noun identifiers.
#![allow(clippy::doc_markdown)]

//! `hello-graph-diff` — R1575 §5.3 §5.50 §5.52 — a link graph drawn in **two
//! layers**: the links a user *authored*, and the links a source *reported*.
//!
//! ## The problem this exists for
//!
//! A graph editor that only draws what the user drew is a diagram. A graph
//! editor that also draws what is actually out there — discovered links, links
//! that came up on their own, links that were authored and never appeared — is
//! a diagnostic instrument, and the whole of its value is in the **difference**
//! between the two layers. That difference is what says "you drew this and it
//! is not there" and "this exists and you did not draw it".
//!
//! Every part of that is cheap except one: the two layers have to be
//! **distinguishable at a glance**, and the convention every tool in this
//! family uses is solid-versus-dashed. pinion could not draw a dashed line at
//! all before this round — [`Stroke`] carried colour, width and cap, and the
//! module doc said dash patterns were carry-forward — so the two-layer view was
//! not a thing a binding could express. R1575 adds [`Dash`], and this binding
//! is its forcing consumer.
//!
//! ## What is derived, and why that is the point
//!
//! [`LinkKind`] is **not stored**. There is no field on a link saying which
//! layer it belongs to, and no code that maintains one. There are two sets —
//! `authored` and `observed` — and the kind of any link is a function of which
//! sets contain it:
//!
//! | in `authored` | in `observed` | kind | drawn |
//! |---|---|---|---|
//! | yes | yes | [`LinkKind::Matched`] | solid |
//! | yes | no | [`LinkKind::Missing`] | dashed, error ink |
//! | no | yes | [`LinkKind::Drift`] | dotted, warning ink |
//!
//! So `invoke adopt` — "make the authored layer say what is actually there" —
//! is one assignment, and every derived fact follows: the counts, the ink, the
//! dash, the accessible description, and the wire. Nothing has to be walked and
//! updated, which is precisely the class of bug a maintained `kind` field
//! exists to produce (two sources of one truth, [[use-substrate-not-hand-rolled-equivalent]]).
//!
//! ## Where this is past Qt
//!
//! Qt draws dashes (`QPen::setDashPattern` / `setDashOffset`), so the *line* is
//! parity. Four things here are not:
//!
//! 1. **The dash is readable.** `scene/snapshot` publishes every path's
//!    `style.stroke.dash` — `null` for solid, otherwise `{on, off, offset,
//!    period}`. A `QPen` is an argument to a paint call: nothing can ask a Qt
//!    scene which of its edges are dashed, so a Qt agent's only route to "is
//!    this link drawn as missing?" is to rasterize and look at pixels. Here the
//!    demo asserts the *paint* against the *model* — two independent readings
//!    of the same fact, which is what makes the drawing checkable at all.
//! 2. **The dash geometry is pixels.** Qt's pattern is in units of the pen
//!    width, so widening a line silently rescales its rhythm. Widening one here
//!    changes the width and nothing else.
//! 3. **A malformed dash is unrepresentable.** `setDashPattern` takes a
//!    `QList<qreal>` that may be odd-length (Qt warns at runtime and ignores
//!    it) or all zeros. [`Dash`]'s `on` / `off` are `NonZeroU32`.
//! 4. **The animation is data.** The marching-ants offset is a declared value
//!    (`intervene flow`, `invoke advance_flow`), canonical modulo the period —
//!    so an agent can step it, read it back, and prove it cycles. Qt's
//!    `dashOffset` is a pen property set inside a paint call and driven by a
//!    `QTimer` nobody outside the process can address.
//!
//! ## The AI surface (§2 #7)
//!
//! The rule this binding is built to satisfy is that **anything a human can
//! conclude from looking at the window, an agent can conclude from the wire**.
//! A human looking at this window can answer: how many links are there, which
//! layer is each in, which ones are missing, which drifted, what happens if I
//! adopt, and is that line dashed. So:
//!
//! * `query link_count` / `matched` / `missing` / `drift` — the census.
//! * `query matched_ids` / `missing_ids` / `drift_ids` / `authored_ids` /
//!   `observed_ids` — *which* ones, not just how many.
//! * `invoke link_kind "a,b"` — one link's layer, by name.
//! * `query flow` + `intervene flow` + `invoke advance_flow` — the animation.
//! * `query scenario` + `intervene scenario` — which observation is loaded.
//! * `invoke adopt` — the verb, answering with what it changed.
//! * `scene/snapshot` — the paint, including each link's dash, tagged
//!   `link.<from>-<to>`.
//!
//! ## Verification
//!
//! `tools/demos/r1575_graph_states_its_layers.py` drives all of it over RPC.

use std::collections::BTreeSet;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    BoxStyle, Color, Dash, LayoutStyle, PathStyle, Size, Stroke, StrokeCap, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

// pinion-forge codegen output: `pub struct HelloGraphDiffRenderer` + its
// error type + async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloGraphDiffRenderer, HelloGraphDiffRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 460;

const VIEW_TAG: &str = "graph_diff";
const THEME_TAG: &str = "app";
const STATE_KEY: &str = "hello-graph-diff/state";

const TITLE_FONT_PX: u32 = 17;
const STATUS_FONT_PX: u32 = 12;
const NODE_FONT_PX: u32 = 12;

const NODE_W: u32 = 96;
const NODE_H: u32 = 34;

/// The nodes, and where they sit. Positions are authored rather than solved:
/// the subject of this binding is the **link layers**, and a layout that moved
/// under a diff would make the demo's paint assertions about the solver.
const NODES: &[(&str, i32, i32)] = &[
    ("hub", 312, 60),
    ("peer-a", 120, 170),
    ("peer-b", 504, 170),
    ("leaf-1", 40, 300),
    ("leaf-2", 240, 300),
    ("leaf-3", 480, 300),
];

/// The links the user drew. Directed: `from` reaches out to `to`.
const AUTHORED: &[(&str, &str)] = &[
    ("peer-a", "hub"),
    ("peer-b", "hub"),
    ("leaf-1", "peer-a"),
    ("leaf-2", "peer-a"),
    ("leaf-3", "peer-b"),
];

/// The observations a source can report. Two of them, because one observation
/// cannot show that the diff *moves*: a demo that only ever loads one would
/// assert the derivation's output rather than that it is a derivation.
///
/// `partial` — `leaf-3` never came up (authored, not observed) and something
/// linked `leaf-2` straight to the hub (observed, not authored).
/// `converged` — exactly the authored set, so a correct diff is empty.
const OBSERVATIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "partial",
        &[
            ("peer-a", "hub"),
            ("peer-b", "hub"),
            ("leaf-1", "peer-a"),
            ("leaf-2", "peer-a"),
            ("leaf-2", "hub"),
        ],
    ),
    ("converged", AUTHORED),
];

/// Which layer (or layers) a link is in.
///
/// Derived on every read from the two sets — see the module doc. The variant
/// order is the reading order of the legend, and [`LinkKind::name`] is the wire
/// vocabulary: three words this binding owns, so a client may match on them
/// (R1565's `data_is_prose` distinction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LinkKind {
    /// Authored and observed. The graph is as drawn.
    Matched,
    /// Authored, not observed — drawn and not there.
    Missing,
    /// Observed, not authored — there and not drawn.
    Drift,
}

impl LinkKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Missing => "missing",
            Self::Drift => "drift",
        }
    }

    /// The ink and the rhythm this kind is drawn with.
    ///
    /// One function, so the paint and the legend cannot disagree about what a
    /// kind looks like — and so the "is the dash the kind says it should be?"
    /// assertion has exactly one thing to be true of.
    fn stroke(self, theme: &pinion_core::theme::Theme) -> Stroke {
        let base = |role, width| Stroke::new(theme.resolve(role), width).with_cap(StrokeCap::Round);
        match self {
            // Solid: the authored graph, confirmed. No dash at all rather than
            // a dash meaning "solid" — see `Stroke::dash`'s doc.
            Self::Matched => base(ColorRole::Accent, 2),
            Self::Missing => base(ColorRole::Error, 2).with_dash(Dash::DASHED),
            Self::Drift => base(ColorRole::OnSurfaceMuted, 2).with_dash(Dash::DOTTED),
        }
    }
}

/// A link, canonical as a `"<from>>?<to>"`-free pair: the id is built by
/// [`link_id`] so one spelling reaches the tag, the wire and the diff.
type Link = (String, String);

fn link_id(from: &str, to: &str) -> String {
    format!("{from}>{to}")
}

/// Parse the `"<from>,<to>"` argument shape the arg-taking reads accept.
fn parse_pair(raw: &str) -> Option<Link> {
    let (from, to) = raw.split_once(',')?;
    Some((from.trim().to_string(), to.trim().to_string()))
}

// --- State --------------------------------------------------------------------

struct DiffState {
    authored: Signal<Vec<Link>>,
    observed: Signal<Vec<Link>>,
    scenario: Signal<String>,
    /// The marching-ants offset in pixels, applied to every dashed link.
    flow: Signal<u32>,
    last_event: Signal<String>,
}

impl DiffState {
    fn new() -> Self {
        let owned = |pairs: &[(&str, &str)]| -> Vec<Link> {
            pairs
                .iter()
                .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
                .collect()
        };
        Self {
            authored: Signal::new(owned(AUTHORED)),
            observed: Signal::new(owned(observation("partial").unwrap_or(&[]))),
            scenario: Signal::new("partial".to_string()),
            flow: Signal::new(0),
            last_event: Signal::new("loaded".to_string()),
        }
    }

    /// The whole diff, in one canonical order.
    ///
    /// Sorted by `(kind, id)` so two reads of an unchanged model are the same
    /// list — the property that lets the demo compare `missing_ids` across
    /// calls without sorting on its side, and the same canonicality argument
    /// `IndexRuns` makes for a selection.
    fn diff(&self) -> Vec<(Link, LinkKind)> {
        let authored: BTreeSet<Link> = self.authored.get().into_iter().collect();
        let observed: BTreeSet<Link> = self.observed.get().into_iter().collect();
        let mut out: Vec<(Link, LinkKind)> = authored
            .union(&observed)
            .map(|link| {
                let kind = match (authored.contains(link), observed.contains(link)) {
                    (true, true) => LinkKind::Matched,
                    (true, false) => LinkKind::Missing,
                    // `union` yields only members of one of the two sets, so
                    // the remaining case is observed-only.
                    (false, _) => LinkKind::Drift,
                };
                (link.clone(), kind)
            })
            .collect();
        out.sort_by(|(la, ka), (lb, kb)| {
            ka.cmp(kb)
                .then_with(|| link_id(&la.0, &la.1).cmp(&link_id(&lb.0, &lb.1)))
        });
        out
    }

    fn count(&self, kind: LinkKind) -> usize {
        self.diff().iter().filter(|(_, k)| *k == kind).count()
    }

    fn ids(&self, kind: LinkKind) -> String {
        self.diff()
            .iter()
            .filter(|(_, k)| *k == kind)
            .map(|((a, b), _)| link_id(a, b))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn set_ids(signal: &Signal<Vec<Link>>) -> String {
        let mut ids: Vec<String> = signal.get().iter().map(|(a, b)| link_id(a, b)).collect();
        ids.sort();
        ids.join(",")
    }
}

fn observation(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    OBSERVATIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, links)| *links)
}

fn use_diff_state() -> Rc<DiffState> {
    Owner::current()
        .expect("use_diff_state requires an active Owner scope")
        .cache(STATE_KEY, DiffState::new)
}

// --- The oracle (primary External) --------------------------------------------

/// Publishes both layers, the derived difference, and the flow offset; owns the
/// two verbs that change them.
struct DiffOracle {
    state: Option<Rc<DiffState>>,
}

impl core::fmt::Debug for DiffOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DiffOracle")
            .field("attached", &self.state.is_some())
            .finish()
    }
}

impl DiffOracle {
    /// R1564 §5.15 — the one sentence for "not wired to a model yet".
    const NO_STATE: &str = "this graph surface is not bound to a model yet";

    fn new() -> Self {
        Self { state: None }
    }

    fn attach_state(&mut self, state: Rc<DiffState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<DiffState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))
    }

    fn text(arg: &IntrospectValue) -> Result<String, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => Ok(s.clone()),
            other => Err(InvokeError::rejected(format!(
                "expected a string argument, got {other:?}"
            ))),
        }
    }
}

impl ExternalIntrospect for DiffOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The census.
                    SchemaField::new("node_count", "int"),
                    SchemaField::new("link_count", "int"),
                    SchemaField::new("matched", "int"),
                    SchemaField::new("missing", "int"),
                    SchemaField::new("drift", "int"),
                    // Which ones — the half a count cannot answer.
                    SchemaField::new("matched_ids", "string"),
                    SchemaField::new("missing_ids", "string"),
                    SchemaField::new("drift_ids", "string"),
                    SchemaField::new("authored_ids", "string"),
                    SchemaField::new("observed_ids", "string"),
                    SchemaField::new("node_names", "string"),
                    SchemaField::new("last_event", "string"),
                    // Writable: which observation is loaded, and the animation.
                    SchemaField::new("scenario", "string"),
                    SchemaField::new("flow", "int"),
                    SchemaField::new("flow_period", "int"),
                    // Arg-taking reads and the verbs.
                    SchemaField::action("link_kind", "string"),
                    SchemaField::action("adopt", "string"),
                    SchemaField::action("advance_flow", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let int = |v: usize| Some(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        let text = |s: String| Some(IntrospectValue::Text(s));
        match path {
            "node_count" => int(NODES.len()),
            "link_count" => int(state.diff().len()),
            "matched" => int(state.count(LinkKind::Matched)),
            "missing" => int(state.count(LinkKind::Missing)),
            "drift" => int(state.count(LinkKind::Drift)),
            "matched_ids" => text(state.ids(LinkKind::Matched)),
            "missing_ids" => text(state.ids(LinkKind::Missing)),
            "drift_ids" => text(state.ids(LinkKind::Drift)),
            "authored_ids" => text(DiffState::set_ids(&state.authored)),
            "observed_ids" => text(DiffState::set_ids(&state.observed)),
            "node_names" => text(
                NODES
                    .iter()
                    .map(|(n, _, _)| *n)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "last_event" => text(state.last_event.get()),
            "scenario" => text(state.scenario.get()),
            "flow" => int(state.flow.get() as usize),
            // Published because it is what `flow` is reduced modulo: without it
            // a client stepping the animation cannot tell when it has come back
            // round, and would have to infer the period from the geometry.
            "flow_period" => int(Dash::DASHED.period() as usize),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let state = self
            .state
            .as_ref()
            .ok_or(InterveneError::UnknownPath)?
            .clone();
        match path {
            "scenario" => {
                let IntrospectValue::Text(name) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let links = observation(name.as_str()).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not an observation; known: {}",
                        OBSERVATIONS
                            .iter()
                            .map(|(n, _)| *n)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })?;
                // `find` matched a static entry, so the name that goes into the
                // signal is the one this binding owns rather than the caller's
                // copy of it.
                let canonical = OBSERVATIONS
                    .iter()
                    .find(|(n, _)| *n == name.as_str())
                    .map_or("partial", |(n, _)| *n);
                state.observed.set(
                    links
                        .iter()
                        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
                        .collect(),
                );
                state.scenario.set(canonical.to_string());
                state.last_event.set(format!("observed {canonical}"));
                Ok(())
            }
            "flow" => {
                let IntrospectValue::Int(px) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let px = u32::try_from(px).map_err(|_| {
                    InterveneError::out_of_range(format!(
                        "a flow offset is a non-negative pixel count, got {px}"
                    ))
                })?;
                // Canonicalised by `Dash::with_offset`, so a write of one full
                // period reads back as 0 rather than as the number sent.
                state.flow.set(Dash::DASHED.with_offset(px).offset);
                state.last_event.set(format!("flow {px}"));
                Ok(())
            }
            "node_count" | "link_count" | "matched" | "missing" | "drift" | "matched_ids"
            | "missing_ids" | "drift_ids" | "authored_ids" | "observed_ids" | "node_names"
            | "last_event" | "flow_period" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "link_kind" => {
                let state = self.state()?;
                let raw = Self::text(&args)?;
                let pair = parse_pair(&raw).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "malformed argument {raw:?} (expected \"<from>,<to>\")"
                    ))
                })?;
                state
                    .diff()
                    .into_iter()
                    .find(|(link, _)| *link == pair)
                    .map(|(_, kind)| IntrospectValue::Text(kind.name().to_string()))
                    .ok_or_else(|| {
                        // Named apart from a malformed argument: the caller
                        // spelled a link correctly and this graph has no such
                        // link in EITHER layer, which is a different fact.
                        InvokeError::rejected(format!(
                            "no link {} in either layer",
                            link_id(&pair.0, &pair.1)
                        ))
                    })
            }
            "adopt" => {
                let state = self.state()?.clone();
                let before = state.count(LinkKind::Missing) + state.count(LinkKind::Drift);
                state.authored.set(state.observed.get());
                state
                    .last_event
                    .set(format!("adopted ({before} differences resolved)"));
                // Answers with what it did, so a caller need not re-read three
                // paths to learn whether anything changed (§7 R1539).
                Ok(IntrospectValue::Int(
                    i64::try_from(before).unwrap_or(i64::MAX),
                ))
            }
            "advance_flow" => {
                let state = self.state()?.clone();
                let px = match &args {
                    IntrospectValue::Int(n) => u32::try_from(*n).map_err(|_| {
                        InvokeError::rejected(format!("a step is non-negative, got {n}"))
                    })?,
                    IntrospectValue::Text(s) if s.is_empty() => 1,
                    IntrospectValue::Text(s) => s
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| InvokeError::rejected(format!("{s:?} is not a pixel step")))?,
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "expected an integer step, got {other:?}"
                        )));
                    }
                };
                let next = Dash::DASHED.with_offset(state.flow.get()).advanced_by(px);
                state.flow.set(next.offset);
                state.last_event.set(format!("flow +{px}"));
                Ok(IntrospectValue::Int(i64::from(next.offset)))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl External for DiffOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
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

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

// --- The view -----------------------------------------------------------------

fn upx(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "view coordinates are < 2^13, exactly representable in f32"
)]
fn ppt(x: i32, y: i32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

fn node_at(name: &str) -> Option<(i32, i32)> {
    NODES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, x, y)| (*x, *y))
}

/// The point a link attaches to on a node box: its centre.
fn anchor(name: &str) -> Option<(i32, i32)> {
    let (x, y) = node_at(name)?;
    Some((
        x + i32::try_from(NODE_W).unwrap_or(0) / 2,
        y + i32::try_from(NODE_H).unwrap_or(0) / 2,
    ))
}

/// One link as a stroked segment, tagged so `scene/snapshot` can be asked what
/// it is drawn with.
fn link_scene(from: &str, to: &str, stroke: Stroke) -> Option<Scene> {
    let (x0, y0) = anchor(from)?;
    let (x1, y1) = anchor(to)?;
    let ox = x0.min(x1);
    let oy = y0.min(y1);
    let bw = (x0.max(x1) - ox).max(1);
    let bh = (y0.max(y1) - oy).max(1);
    let rect = Rect::new(upx(ox), upx(oy), upx(bw), upx(bh));
    let (org_x, org_y) = (
        i32::try_from(rect.x).unwrap_or(0),
        i32::try_from(rect.y).unwrap_or(0),
    );
    let commands = vec![
        PathCommand::MoveTo(ppt(x0 - org_x, y0 - org_y)),
        PathCommand::LineTo(ppt(x1 - org_x, y1 - org_y)),
    ];
    Some(Scene::Path(
        PathNode::new(rect, commands, PathStyle::stroked(stroke))
            .with_tag(format!("link.{from}-{to}"))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h)),
            ),
    ))
}

fn node_scene(name: &str, x: i32, y: i32, fill: Color, ink: Color, outline: Color) -> Scene {
    let rect = Rect::new(upx(x), upx(y), NODE_W, NODE_H);
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            name,
            Rect::new(upx(x) + 10, upx(y) + 9, NODE_W - 20, NODE_FONT_PX + 4),
            TextStyle::new().with_size_px(NODE_FONT_PX).with_fg(ink),
        ))])
        .with_tag(format!("node.{name}"))
        .with_style(
            BoxStyle::filled(fill)
                .with_corner_radius(8)
                .with_border(pinion_core::style::Border::new(outline, 1)),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    )
}

fn status_text(state: &DiffState) -> String {
    format!(
        "observation \"{}\" — {} matched · {} missing (dashed) · {} drift (dotted) · flow {}px",
        state.scenario.get(),
        state.count(LinkKind::Matched),
        state.count(LinkKind::Missing),
        state.count(LinkKind::Drift),
        state.flow.get(),
    )
}

fn view(_state: (), _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_diff_state();
    let flow = state.flow.get();

    let mut children = vec![
        Scene::Text(TextNode::styled(
            "Authored links against observed links — the difference is derived, not stored",
            Rect::new(24, 20, WIN_W - 48, TITLE_FONT_PX + 4),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )),
        Scene::Text(TextNode::styled(
            status_text(&state),
            Rect::new(24, 44, WIN_W - 48, STATUS_FONT_PX + 4),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )),
    ];

    // Links first, so a node box always paints over the line reaching it.
    for ((from, to), kind) in state.diff() {
        let stroke = kind.stroke(&theme);
        // The flow offset applies only where there IS a dash: advancing a solid
        // stroke is not a thing, which is the shape `Option<Dash>` makes true
        // rather than merely conventional.
        let stroke = match stroke.dash {
            Some(dash) => stroke.with_dash(dash.with_offset(flow)),
            None => stroke,
        };
        if let Some(scene) = link_scene(&from, &to, stroke) {
            children.push(scene);
        }
    }

    let fill = theme.resolve(ColorRole::SurfaceContainerHigh);
    let ink = theme.resolve(ColorRole::OnSurface);
    let outline = theme.resolve(ColorRole::Outline);
    for (name, x, y) in NODES {
        children.push(node_scene(name, *x, *y, fill, ink, outline));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct GraphDiffView;

impl WidgetCore for GraphDiffView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = DiffOracle::new();
        oracle.attach_state(use_diff_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, *frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-graph-diff (R1575 §5.3 two-layer link graph)"
    }
}

impl WidgetA11y for GraphDiffView {
    /// The view is a WAI-ARIA `img` whose value text says what the drawing
    /// says — including which links are missing and which drifted, by name.
    ///
    /// This is the reading a sighted user gets from solid-versus-dashed, and it
    /// is why the dash had to become a declaration rather than a paint detail:
    /// a `QPen` dash reaches the screen and nothing else, so the same
    /// distinction in Qt is invisible to assistive technology by construction.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_diff_state();
        let describe = |kind: LinkKind| {
            let ids = state.ids(kind);
            if ids.is_empty() {
                format!("no {} links", kind.name())
            } else {
                format!("{} {}", kind.name(), ids.replace(',', ", "))
            }
        };
        vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Authored and observed link graph")
                .with_value(AccessValue::Text(format!(
                    "{} nodes, {} links under observation \"{}\": {}; {}; {}",
                    NODES.len(),
                    state.diff().len(),
                    state.scenario.get(),
                    describe(LinkKind::Matched),
                    describe(LinkKind::Missing),
                    describe(LinkKind::Drift),
                ))),
        ]
    }
}

impl WidgetView for GraphDiffView {
    type Renderer = HelloGraphDiffRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GraphDiffView>();
}

#[cfg(test)]
mod tests;
