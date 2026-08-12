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
//! [`LinkLayer`] is **not stored**. There is no field on a link saying which
//! layer it belongs to, and no code that maintains one. There are two layers —
//! the tree's links and [`Document::observe`]'s reports — and the layer of any
//! link is a function of which of them contain it:
//!
//! | in `authored` | in `observed` | kind | drawn |
//! |---|---|---|---|
//! | yes | yes | [`LinkLayer::Matched`] | solid |
//! | yes | no | [`LinkLayer::Missing`] | dashed, error ink |
//! | no | yes | [`LinkLayer::Drift`] | dotted, warning ink |
//!
//! So `invoke adopt` — "make the drawn layer say what is actually there" —
//! runs [`Document::adopt`] per reported link and every derived fact follows:
//! the counts, the ink, the dash, the accessible description, and the wire.
//! Nothing has to be walked and updated, which is precisely the class of bug a
//! maintained `kind` field exists to produce (two sources of one truth).
//!
//! ## R1645 — the model is the crate's now
//!
//! This binding kept two `Vec` of name pairs until `pinion-node-graph` had
//! somewhere to put a reported link. It no longer does, and three things follow
//! that a pair of sets cannot produce: adoption runs the **authoring rules**,
//! so a report closing a cycle is *named* rather than assigned; `standing` says
//! whether the drawing can be read as the topology at all; and `reaches`
//! answers on **both** layers, so the case where a static rule says blocked and
//! the world disagrees is one read.
//!
//! ## Where this is past the toolkit
//!
//! The toolkit draws dashes (`setDashPattern` / `setDashOffset`), so the *line* is parity. Four things
//! here are not:
//!
//! 1. **The dash is readable.** `scene/snapshot` publishes every path's
//!    `style.stroke.dash` — `null` for solid, otherwise `{on, off, offset,
//!    period}`. A pen is an argument to a paint call: nothing can ask a toolkit
//!    scene which of its edges are dashed, so a toolkit agent's only route to "is
//!    this link drawn as missing?" is to rasterize and look at pixels. Here the
//!    demo asserts the *paint* against the *model* — two independent readings
//!    of the same fact, which is what makes the drawing checkable at all.
//! 2. **The dash geometry is pixels.** the toolkit's pattern is in units of the pen
//!    width, so widening a line silently rescales its rhythm. Widening one here
//!    changes the width and nothing else.
//! 3. **A malformed dash is unrepresentable.** `setDashPattern` takes a
//!    `list<qreal>` that may be odd-length (the toolkit warns at runtime and ignores
//!    it) or all zeros. [`Dash`]'s `on` / `off` are `NonZeroU32`.
//! 4. **The animation is data.** The marching-ants offset is a declared value
//!    (`intervene flow`, `invoke advance_flow`), canonical modulo the period —
//!    so an agent can step it, read it back, and prove it cycles. The toolkit's
//!    `dashOffset` is a pen property set inside a paint call and driven by a
//!    timer nobody outside the process can address.
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

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    BoxStyle, Color, Dash, LayoutStyle, PathStyle, Size, Stroke, StrokeCap, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_node_graph::{
    AdoptError, Discovery, Document, LinkLayer, NodeBody, NodeId, NodeKind, Port, ROOT, Socket,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use serde::{Deserialize, Serialize};

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
    ("impossible", IMPOSSIBLE),
];

/// A third observation, so the crate's own refusal is reachable: the world
/// reports a link back from the hub, which closes a cycle this model will not
/// hold (R1645).
///
/// Adopting it is refused BY THE AUTHORING RULE, and the refusal is the finding
/// — "this exists out there and your drawing cannot express it". A binding
/// keeping its own two sets of name pairs could not produce it at all, because
/// nothing in a set of pairs knows what a cycle is.
const IMPOSSIBLE: &[(&str, &str)] = &[
    ("peer-a", "hub"),
    ("peer-b", "hub"),
    ("leaf-1", "peer-a"),
    ("leaf-2", "peer-a"),
    ("leaf-3", "peer-b"),
    ("hub", "peer-a"),
];

/// The taxonomy: one kind, carrying the node's name.
///
/// Four inputs because a topology node takes several feeds and a value input
/// takes one link each; one output because a value output feeds as many as it
/// likes. The kind computes nothing — this graph's subject is its *shape*.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
struct Named(String);

impl NodeKind for Named {
    type Type = ();
    type Value = ();

    fn name(&self) -> String {
        self.0.clone()
    }

    fn inputs(&self) -> Vec<Port<(), ()>> {
        (0..4).map(|n| Port::new(format!("in {n}"), ())).collect()
    }

    fn outputs(&self) -> Vec<Port<(), ()>> {
        vec![Port::new("out", ())]
    }

    fn evaluate(&self, _inputs: &[Option<()>]) -> Vec<Option<()>> {
        vec![None]
    }
}

/// The ink and the rhythm a layer is drawn with.
///
/// One function, so the paint and the legend cannot disagree about what a
/// layer looks like — and so the "is the dash the layer says it should be?"
/// assertion has exactly one thing to be true of.
fn layer_stroke(layer: LinkLayer, theme: &pinion_core::theme::Theme) -> Stroke {
    let base = |role, width| Stroke::new(theme.resolve(role), width).with_cap(StrokeCap::Round);
    match layer {
        // Solid: the authored graph, confirmed. No dash at all rather than
        // a dash meaning "solid" — see `Stroke::dash`'s doc.
        LinkLayer::Matched => base(ColorRole::Accent, 2),
        LinkLayer::Missing => base(ColorRole::Error, 2).with_dash(Dash::DASHED),
        LinkLayer::Drift => base(ColorRole::OnSurfaceMuted, 2).with_dash(Dash::DOTTED),
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
    /// R1645 — ONE model, holding both layers.
    ///
    /// This binding used to keep two `Vec<Link>` of its own and derive the
    /// layers by set membership. The derivation was right and it is now the
    /// crate's ([`Document::layers`]), which is what makes this an application
    /// of a node system rather than a second one: the authored layer is real
    /// links, so `connect`'s rules apply to it, and the reported layer is
    /// [`Document::observe`], which no derivation in the crate walks.
    document: Signal<Document<Named>>,
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
        let _ = owned;
        Self {
            document: Signal::new(build("partial")),
            scenario: Signal::new("partial".to_string()),
            flow: Signal::new(0),
            last_event: Signal::new("loaded".to_string()),
        }
    }

    /// The whole diff, in one canonical order — now a projection of
    /// [`Document::layers`] back onto the names this screen paints (R1645).
    ///
    /// Sorted by `(layer, id)` so two reads of an unchanged model are the same
    /// list — the property that lets the demo compare `missing_ids` across
    /// calls without sorting on its side.
    fn diff(&self) -> Vec<(Link, LinkLayer)> {
        let document = self.document.get();
        let layers = document.layers(ROOT);
        let mut out: Vec<(Link, LinkLayer)> = Vec::new();
        for (ids, layer) in [
            (layers.matched(), LinkLayer::Matched),
            (layers.missing(), LinkLayer::Missing),
        ] {
            for id in ids {
                if let Some(link) = document.tree(ROOT).and_then(|t| t.link(*id)) {
                    out.push((named_pair(&document, link.from, link.to), layer));
                }
            }
        }
        for seen in layers.drift() {
            out.push((named_pair(&document, seen.from, seen.to), LinkLayer::Drift));
        }
        out.sort_by(|(la, ka), (lb, kb)| {
            ka.cmp(kb)
                .then_with(|| link_id(&la.0, &la.1).cmp(&link_id(&lb.0, &lb.1)))
        });
        out
    }

    fn count(&self, layer: LinkLayer) -> usize {
        self.diff().iter().filter(|(_, k)| *k == layer).count()
    }

    fn ids(&self, layer: LinkLayer) -> String {
        self.diff()
            .iter()
            .filter(|(_, k)| *k == layer)
            .map(|((a, b), _)| link_id(a, b))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The ids of one whole layer of the model, drawn or reported.
    fn layer_ids(&self, drawn: bool) -> String {
        let document = self.document.get();
        let mut ids: Vec<String> = if drawn {
            document
                .tree(ROOT)
                .map(|t| {
                    t.links()
                        .iter()
                        .map(|l| {
                            let (a, b) = named_pair(&document, l.from, l.to);
                            link_id(&a, &b)
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            document
                .observations(ROOT)
                .into_iter()
                .map(|seen| {
                    let (a, b) = named_pair(&document, seen.from, seen.to);
                    link_id(&a, &b)
                })
                .collect()
        };
        ids.sort();
        ids.join(",")
    }
}

/// The two node names a socket pair sits on.
fn named_pair(document: &Document<Named>, from: Socket, to: Socket) -> Link {
    let name = |node| {
        document
            .tree(ROOT)
            .and_then(|t| t.node(node))
            .map_or_else(String::new, pinion_node_graph::Node::display_name)
    };
    (name(from.node), name(to.node))
}

/// The whole model for one scenario: the drawn links, and what a source
/// reported.
fn build(scenario: &str) -> Document<Named> {
    let mut document = Document::new("topology");
    let mut ids: Vec<(&str, NodeId)> = Vec::new();
    for (name, x, y) in NODES {
        let node = document
            .add_node(ROOT, NodeBody::Kind(Named((*name).to_string())), *x, *y)
            .expect("the root tree takes a node");
        ids.push((name, node));
    }
    let of = |name: &str| {
        ids.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, id)| *id)
            .expect("every link names a node in NODES")
    };
    // A value input takes one link, so each arrival gets its own free port —
    // which is what a topology node's several feeds are.
    for (from, to) in AUTHORED {
        let sink = of(to);
        let port = free_input(&document, sink);
        document
            .connect(ROOT, Socket::new(of(from), 0), Socket::new(sink, port))
            .expect("the drawn topology is a legal graph");
    }
    for (from, to) in observation(scenario).unwrap_or(&[]) {
        let sink = of(to);
        // A report lands on the port the drawn link uses when there is one, so
        // a link that was drawn AND seen is one pair rather than two.
        let port = document
            .tree(ROOT)
            .and_then(|t| {
                t.links()
                    .iter()
                    .find(|l| l.from.node == of(from) && l.to.node == sink)
            })
            .map_or_else(|| free_input(&document, sink), |l| l.to.port);
        document
            .observe(ROOT, Socket::new(of(from), 0), Socket::new(sink, port))
            .expect("a report about this graph");
    }
    document
}

/// The first input of `node` nothing is wired to.
fn free_input(document: &Document<Named>, node: NodeId) -> u32 {
    let host = document.tree(ROOT).expect("the root tree");
    (0..4)
        .find(|port| host.link_into(Socket::new(node, *port)).is_none())
        .unwrap_or(0)
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
    /// R1645 — does one node reach another, on each layer?
    ///
    /// Its own function because the dispatcher is at the line ceiling, and
    /// because this is the one read here that is about the two layers
    /// DISAGREEING rather than about either of them.
    fn reaches(
        state: &Rc<DiffState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(raw) = args else {
            return Err(InvokeError::rejected("expected \"<from>,<to>\""));
        };
        let pair = parse_pair(raw)
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <from>,<to>")))?;
        let document = state.document.get();
        let of = |name: &str| {
            document
                .tree(ROOT)
                .and_then(|t| t.nodes().find(|n| n.display_name() == name).map(|n| n.id))
        };
        let (Some(from), Some(to)) = (of(&pair.0), of(&pair.1)) else {
            return Err(InvokeError::rejected(format!(
                "no node named {:?} or {:?}",
                pair.0, pair.1
            )));
        };
        let judged = document.reaches(ROOT, from, to);
        // The standing travels WITH the answer, so a client cannot read a
        // partial drawing's verdict as a fact about the world.
        Ok(IntrospectValue::Text(format!(
            "drawn={} observed={} disagrees={} standing={}",
            judged.answer().drawn,
            judged.answer().observed,
            judged.answer().disagrees(),
            judged.standing().name(),
        )))
    }

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
                    // R1645 — the standing: whether an answer computed from
                    // the drawn links is an answer about the world.
                    SchemaField::new("standing", "string"),
                    SchemaField::new("certain", "string"),
                    SchemaField::new("discovery", "string"),
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
                    // R1645 — does one node reach another, on EACH layer? The
                    // case that matters is the disagreement: a static rule says
                    // blocked and the world says otherwise.
                    SchemaField::action_with(
                        "reaches",
                        "string",
                        ArgForm::Delimited(','),
                        const {
                            &[
                                SchemaArg::key("from", "string", "node_names"),
                                SchemaArg::key("to", "string", "node_names"),
                            ]
                        },
                    ),
                    SchemaField::action("link_kind", "string"),
                    SchemaField::action("adopt", "string"),
                    SchemaField::action("advance_flow", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("no diff has been computed yet"))?;
        let int = |v: usize| Ok(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        let text = |s: String| Ok(IntrospectValue::Text(s));
        match path {
            "node_count" => int(NODES.len()),
            "link_count" => int(state.diff().len()),
            "matched" => int(state.count(LinkLayer::Matched)),
            "missing" => int(state.count(LinkLayer::Missing)),
            "drift" => int(state.count(LinkLayer::Drift)),
            "matched_ids" => text(state.ids(LinkLayer::Matched)),
            "missing_ids" => text(state.ids(LinkLayer::Missing)),
            "drift_ids" => text(state.ids(LinkLayer::Drift)),
            "authored_ids" => text(state.layer_ids(true)),
            "observed_ids" => text(state.layer_ids(false)),
            "node_names" => text(
                NODES
                    .iter()
                    .map(|(n, _, _)| *n)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "last_event" => text(state.last_event.get()),
            "scenario" => text(state.scenario.get()),
            // R1645 — three reads a binding keeping two sets of name pairs
            // could not answer, because none of them is about the sets: they
            // are about whether the drawing can be READ as the topology.
            "standing" => text(state.document.get().standing(ROOT).to_string()),
            "discovery" => text(state.document.get().discovery().name().to_owned()),
            "certain" => text(
                if state.document.get().standing(ROOT).is_certain() {
                    "yes"
                } else {
                    "no"
                }
                .to_owned(),
            ),
            "flow" => int(state.flow.get() as usize),
            // Published because it is what `flow` is reduced modulo: without it
            // a client stepping the animation cannot tell when it has come back
            // round, and would have to infer the period from the geometry.
            "flow_period" => int(Dash::DASHED.period() as usize),
            _ => Err(ReadRefusal::UnknownPath),
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
                let _ = links;
                // R1645 — one model, rebuilt for the scenario: the drawn links
                // and the reports arrive together, and the difference between
                // them is derived rather than assigned.
                state.document.set(build(canonical));
                state.scenario.set(canonical.to_string());
                state.last_event.set(format!("observed {canonical}"));
                Ok(())
            }
            // R1645 — the determinism switch, and the one writable slot here.
            "discovery" => {
                let IntrospectValue::Text(word) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let chosen = Discovery::from_wire(word.trim()).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "discovery is one of {}, got {word:?}",
                        Discovery::WIRE_NAMES.join(" / ")
                    ))
                })?;
                let mut document = state.document.get();
                let was = document.set_discovery(chosen);
                state.document.set(document);
                state
                    .last_event
                    .set(format!("discovery {} (was {})", chosen.name(), was.name()));
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
            | "last_event" | "flow_period" | "standing" | "certain" => {
                Err(InterveneError::ReadOnly)
            }
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
            "reaches" => Self::reaches(&self.state()?.clone(), &args),
            "adopt" => {
                let state = self.state()?.clone();
                let before = state.count(LinkLayer::Missing) + state.count(LinkLayer::Drift);
                // R1645 — adoption runs the AUTHORING rules, one reported link
                // at a time, and a report this model cannot hold is named
                // rather than swallowed. That refusal is the finding: the world
                // is doing something the drawing cannot express. Assigning one
                // set to the other — which is what this binding did before the
                // crate held both layers — could not produce it.
                let mut document = state.document.get();
                let drift = document.layers(ROOT).drift().to_vec();
                let mut refused: Vec<String> = Vec::new();
                for seen in drift {
                    if let Err(AdoptError::CannotAuthor(why)) =
                        document.adopt(ROOT, seen.from, seen.to)
                    {
                        let (a, b) = named_pair(&document, seen.from, seen.to);
                        refused.push(format!("{}: {why}", link_id(&a, &b)));
                    }
                }
                // A drawn link nobody reported is retracted, which is the other
                // half of "make the drawing say what is there".
                for id in document.layers(ROOT).missing().to_vec() {
                    document.disconnect(ROOT, id).ok();
                }
                state.document.set(document);
                if !refused.is_empty() {
                    state
                        .last_event
                        .set(format!("refused {}", refused.join("; ")));
                    return Err(InvokeError::rejected(refused.join("; ")));
                }
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

/// The outline a node strokes inside its own box.
const NODE_FRAME: u32 = 1;

/// The clearance a node's name keeps from its frame.
const NODE_PAD_X: u32 = 10;

fn node_scene(name: &str, x: i32, y: i32, fill: Color, ink: Color, outline: Color) -> Scene {
    let rect = Rect::new(upx(x), upx(y), NODE_W, NODE_H);
    // ★★ R1673 — the name is placed by the FLOW inside a reserved frame, not by
    // a rect that reads like a position and is not one.
    //
    // It was authored at `(x + 10, y + 9)` in WINDOW coordinates on a text node
    // whose container is already absolutely placed — and a `TextNode`'s rect is
    // its box, not its origin (R1653's lesson, one screen over). So every one of
    // this screen's six names was laid at the container's flow origin, which is
    // the box corner, and covered the node's own outline on two edges.
    let line = pinion_core::containment::line_box(NODE_FONT_PX);
    let inset = NODE_FRAME + NODE_PAD_X;
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            name,
            Rect::new(0, 0, NODE_W.saturating_sub(inset * 2), line),
            TextStyle::new().with_size_px(NODE_FONT_PX).with_fg(ink),
        ))])
        .with_tag(format!("node.{name}"))
        .with_style(
            BoxStyle::filled(fill)
                .with_corner_radius(8)
                .with_border(pinion_core::style::Border::new(outline, NODE_FRAME)),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h))
                .with_padding(Rect::new(
                    inset,
                    NODE_H.saturating_sub(line) / 2,
                    inset,
                    NODE_H.saturating_sub(line) / 2,
                )),
        ),
    )
}

fn status_text(state: &DiffState) -> String {
    format!(
        "observation \"{}\" — {} matched · {} missing (dashed) · {} drift (dotted) · flow {}px",
        state.scenario.get(),
        state.count(LinkLayer::Matched),
        state.count(LinkLayer::Missing),
        state.count(LinkLayer::Drift),
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
        let stroke = layer_stroke(kind, &theme);
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
    /// This is the reading a sighted user gets from solid-versus-dashed, and
    /// it is why the dash had to become a declaration rather than a paint
    /// detail: a pen dash reaches the screen and nothing else, so the same
    /// distinction in the toolkit is invisible to assistive technology by
    /// construction.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_diff_state();
        let describe = |kind: LinkLayer| {
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
                    describe(LinkLayer::Matched),
                    describe(LinkLayer::Missing),
                    describe(LinkLayer::Drift),
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
