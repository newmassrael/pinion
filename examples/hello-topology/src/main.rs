//! `hello-topology` — R1442 §5.38 §5.52: a **live service-topology view**, and
//! the second consumer of the layered graph solver.
//!
//! ## What this demonstrates
//!
//! `hello-node-editor` (R838…R1441) is an *editor*: a human drags nodes, and
//! `auto_layout` is a verb they choose to press. A topology **view** is the other
//! half of the same problem. The graph arrives from somewhere else — a service
//! mesh, a dependency scan, a live discovery feed — carrying no coordinates at
//! all, so the tool has to invent them, and invent them again every single time
//! the graph changes.
//!
//! That is what makes this the round's forcing consumer. A one-shot layout
//! minimises crossings, which is exactly right once. Run it again after one new
//! service appears and it is free to re-order columns that had nothing to do
//! with the change: the whole picture jumps, and the viewer has to re-learn a
//! drawing they had already learned. The literature's name for what is lost is
//! the **mental map** (Misue, Eades, Lai & Sugiyama, JVLC 1995).
//!
//! So [`pinion_graph::Sugiyama`] grew a second ordering — seed each column from
//! the PREVIOUS drawing's coordinates instead of from crossing minimisation —
//! and this view drives all three, live:
//!
//! * `stable` — nothing the viewer has already seen is reordered; only new
//!   services have to find a slot.
//! * `fresh` — every relayout re-minimises crossings from scratch, the editor's
//!   `auto_layout` on a timer.
//! * `settled` (R1443) — the seeded order, then the exchanges that strictly
//!   remove a crossing and no others.
//!
//! **Neither of the first two is free, and the demo's point is that the cost is
//! measurable in both directions.** Stability is paid for in `crossings`;
//! tidiness is paid for in `order_changes` — the number of remembered pairs a
//! drawing flipped. Both are published on the introspect surface for the SAME
//! pass, so an agent reads the trade rather than being told about it (§2 #7).
//!
//! ## The tangle a stable view could not shed (R1443)
//!
//! R1442 shipped with a hole its own metrics made visible: a `stable` view keeps
//! every crossing its changes introduce, for ever, and the only relief on offer
//! was `fresh` — which throws the whole learned picture away to remove them. So
//! there was no answer to "this has got messy, tidy it up a bit".
//!
//! There is now, and it is two things rather than one, because they are
//! different in kind. `settled` is a **policy**: draw every change that way and
//! the view never accumulates a tangle in the first place. `untangle` is a
//! **verb**: stay stable, and relieve the tangle when the viewer asks, leaving
//! the mode alone. Both run the same pass, and it reports its cost in the same
//! two currencies, so the three orderings are comparable on one graph.
//!
//! ## Wires go where the layout put them
//!
//! A long dependency — `edge -> db`, jumping three columns — is not drawn as a
//! diagonal across whatever cards are in the way. The solver reserved it a slot
//! in every column it crosses (R1441's bends), and `Layout::route` now says
//! where those slots are, so the wire is a polyline through them. The middle of
//! that run is straight because the coordinate solver's guarantee is about
//! exactly those segments, and `straight_inner` / `inner_segments` report it.
//!
//! ## Live without a clock
//!
//! The feed is a scripted incident (`advance` applies the next step) rather than
//! a producer thread, so every assertion in `tools/demos/r1442_live_topology.py`
//! is deterministic (ZERO-FLAKE). Nothing about the reducer is script-specific:
//! a real producer calls the same `add_service` / `connect` from a
//! `RepaintSink`, which `hello-live-chart` (R1398) already proves.
//!
//! ## Qt reference
//!
//! Qt has no graph layout at all — `QGraphicsScene` draws what you position, so
//! a topology view there is Graphviz-out-of-process or a hand-rolled solver, and
//! neither gives the application a stability contract. Graphviz `dot` has no
//! incremental mode; ELK does (`INTERACTIVE` crossing minimisation), and this is
//! that strategy with the numbers on the wire.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{
    BoxNode, ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode,
};
use pinion_core::style::{
    Border, BoxStyle, Color, LayoutStyle, PathStyle, Size, Stroke, StrokeCap, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_graph::Sugiyama;
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTopologyRenderer, HelloTopologyRendererError);

const THEME_TAG: &str = "app";
/// The view's registration tag — addressed over RPC as `/external/<field>`.
const VIEW_TAG: &str = "topology";
/// Reactive-cache key for the shared model.
const STATE_KEY: &str = "topology-state";

const WIN_W: u32 = 880;
const WIN_H: u32 = 520;
/// Left edge of the first column, and the graph's top.
const ORIGIN: (i32, i32) = (28, 104);
const CARD_W: i32 = 108;
const CARD_H: i32 = 36;
/// Horizontal clearance between two columns — the channel a long wire runs in.
const COL_GAP: i32 = 58;
const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;
const LABEL_FONT_PX: u32 = 13;

/// The layered pass this view runs. `row_gap` is generous because a topology is
/// read at a glance rather than edited, and `bend_size` keeps two wires crossing
/// one column separable.
const LAYOUT: Sugiyama = Sugiyama {
    row_gap: 22,
    bend_size: 10,
    sweeps: 4,
};

// --- The model ---------------------------------------------------------------

/// One service. The id is minted once and never reused, so a drawing recorded
/// against it survives other services being removed — the same stable-handle
/// discipline the node editor uses for its nodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Service {
    id: u32,
    name: String,
}

/// One dependency: `from` calls `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Link {
    from: u32,
    to: u32,
}

/// The graph as it currently stands. No coordinates live here — this is what
/// arrives over the wire, and placing it is the view's job.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Topology {
    services: Vec<Service>,
    links: Vec<Link>,
    next_id: u32,
}

impl Topology {
    fn id_of(&self, name: &str) -> Option<u32> {
        self.services.iter().find(|s| s.name == name).map(|s| s.id)
    }

    fn name_of(&self, id: u32) -> &str {
        self.services
            .iter()
            .find(|s| s.id == id)
            .map_or("", |s| s.name.as_str())
    }

    /// Add a service, or `None` if that name is already known — a topology is
    /// keyed by name, and two services with one name is a discovery bug, not a
    /// second node.
    fn add(&mut self, name: &str) -> Option<u32> {
        if self.id_of(name).is_some() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.services.push(Service {
            id,
            name: name.to_string(),
        });
        Some(id)
    }

    /// Remove a service **and every dependency touching it** — a dangling link
    /// would name a service that is not there, which no layout can place.
    fn remove(&mut self, name: &str) -> bool {
        let Some(id) = self.id_of(name) else {
            return false;
        };
        self.services.retain(|s| s.id != id);
        self.links.retain(|l| l.from != id && l.to != id);
        true
    }

    fn connect(&mut self, from: &str, to: &str) -> bool {
        let (Some(from), Some(to)) = (self.id_of(from), self.id_of(to)) else {
            return false;
        };
        let link = Link { from, to };
        if from == to || self.links.contains(&link) {
            return false;
        }
        self.links.push(link);
        true
    }

    fn disconnect(&mut self, from: &str, to: &str) -> bool {
        let (Some(from), Some(to)) = (self.id_of(from), self.id_of(to)) else {
            return false;
        };
        let before = self.links.len();
        self.links.retain(|l| l.from != from || l.to != to);
        self.links.len() != before
    }
}

/// The service mesh the view starts with.
///
/// Two properties are authored deliberately, and the tests assert both rather
/// than trusting this comment:
///
/// * `gw-eu -> warehouse` spans three columns, so it is the long edge whose
///   reserved channel the wire router has to use.
/// * the middle column's three services have DIFFERENT upstreams (`api` from
///   the EU gateway, `search` from the US one, `auth` from both), so their
///   barycenters differ. A column whose members all hang off the same node ties
///   on every sweep and is then ordered by index — which looks stable but is
///   only arithmetic, and would credit the seeded ordering with a stability it
///   did not have to work for.
fn seed_topology() -> Topology {
    let mut topology = Topology::default();
    for name in ["gw-eu", "gw-us", "api", "auth", "search", "db", "warehouse"] {
        topology.add(name);
    }
    for (from, to) in [
        ("gw-eu", "api"),
        ("gw-eu", "auth"),
        ("gw-us", "auth"),
        ("gw-us", "search"),
        ("api", "db"),
        ("auth", "db"),
        ("search", "db"),
        ("db", "warehouse"),
        ("gw-eu", "warehouse"),
    ] {
        topology.connect(from, to);
    }
    topology
}

/// One step of the scripted feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Add(&'static str),
    Remove(&'static str),
    Connect(&'static str, &'static str),
    Disconnect(&'static str, &'static str),
}

impl Step {
    fn describe(self) -> String {
        match self {
            Self::Add(name) => format!("+ {name} appeared"),
            Self::Remove(name) => format!("- {name} went away"),
            Self::Connect(from, to) => format!("+ {from} -> {to}"),
            Self::Disconnect(from, to) => format!("- {from} -> {to}"),
        }
    }
}

/// The incident: a cache is introduced in front of the database, a third region
/// comes online behind the API, and the auth service is retired.
///
/// Every step changes the SHAPE of the graph — a column gains a member, a
/// column is created, a column empties — so a seeded relayout has something to
/// hold on to. Step 6 is the one that makes the CONTRAST real: a new upstream
/// changes `api`'s barycenter, so a fresh pass slides it past a service the
/// viewer had already placed. A feed without such a step would let a fresh
/// layout look stable for arithmetic reasons and prove nothing.
const STREAM: [Step; 7] = [
    Step::Add("cache"),
    Step::Connect("api", "cache"),
    Step::Connect("cache", "db"),
    // The point of a cache: `api` stops talking to the database directly.
    Step::Disconnect("api", "db"),
    Step::Add("gw-ap"),
    Step::Connect("gw-ap", "api"),
    Step::Remove("auth"),
];

/// Which ordering a relayout uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    /// Keep the drawing the viewer already has.
    Stable,
    /// Re-minimise crossings from scratch.
    Fresh,
    /// R1443 — keep it, except where an exchange strictly removes a crossing.
    Settled,
}

impl Mode {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "stable" => Some(Self::Stable),
            "fresh" => Some(Self::Fresh),
            "settled" => Some(Self::Settled),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Fresh => "fresh",
            Self::Settled => "settled",
        }
    }
}

// --- The drawing --------------------------------------------------------------

/// What one relayout cost, in both currencies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Stats {
    /// Columns in the drawing.
    depth: usize,
    /// Edge crossings — what a tidy drawing minimises.
    crossings: usize,
    /// Remembered pairs this drawing flipped — what a stable drawing keeps at 0.
    order_changes: usize,
    /// Inner segments, and how many of them run straight.
    inner: usize,
    straight: usize,
    /// Bends inserted for long edges — the reserved wire channels.
    bends: usize,
}

/// A placed topology: where every card sits, where every wire runs, and what the
/// pass cost. `centres` is the seed the NEXT relayout is measured and ordered
/// against, which is why it is kept beside the pixels rather than recomputed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Drawing {
    /// Card top-left, by service id.
    cards: BTreeMap<u32, (i32, i32)>,
    /// Free-axis CENTRE in solver space, by service id — the next pass's seed.
    centres: BTreeMap<u32, i32>,
    /// Column, by service id.
    columns: BTreeMap<u32, usize>,
    /// Every link's polyline in view coordinates, in `Topology::links` order.
    wires: Vec<Vec<(i32, i32)>>,
    stats: Stats,
}

/// The x of column `column`'s left edge.
fn column_x(column: usize) -> i32 {
    ORIGIN.0 + i32::try_from(column).unwrap_or(0) * (CARD_W + COL_GAP)
}

/// **The reducer.** Lay `topology` out, seeding from `previous` (the drawing the
/// viewer currently has) when `mode` asks for any of the stability it offers.
///
/// A first drawing has nothing to preserve, so an empty seed always takes the
/// fresh path whatever the mode says — seeding on nothing would just be index
/// order with the crossing reduction switched off, and that is true of
/// [`Mode::Settled`] too: with no remembered order there is no cheaper move than
/// the tidiest one.
fn relayout(topology: &Topology, previous: &BTreeMap<u32, i32>, mode: Mode) -> Drawing {
    let mut ids: Vec<u32> = topology.services.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    let slot: BTreeMap<u32, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let sizes = vec![CARD_H; ids.len()];
    // Index pairs in `topology.links` order, so `Layout::route(i)` answers for
    // link `i` and the wire cannot be routed onto some other edge's channel.
    let edges: Vec<(usize, usize)> = topology
        .links
        .iter()
        .map(|l| (slot[&l.from], slot[&l.to]))
        .collect();

    let seed: Vec<Option<i32>> = ids.iter().map(|id| previous.get(id).copied()).collect();
    let remembered = seed.iter().any(Option::is_some);
    let layout = match mode {
        Mode::Stable if remembered => LAYOUT.run_seeded(&sizes, &edges, &seed),
        Mode::Settled if remembered => LAYOUT.run_settled(&sizes, &edges, &seed),
        _ => LAYOUT.run(&sizes, &edges),
    };

    let top = layout.top();
    let mut cards = BTreeMap::new();
    let mut centres = BTreeMap::new();
    let mut columns = BTreeMap::new();
    for (i, &id) in ids.iter().enumerate() {
        let column = layout.layers()[i];
        let centre = layout.centres()[i];
        cards.insert(id, (column_x(column), ORIGIN.1 + centre - top - CARD_H / 2));
        centres.insert(id, centre);
        columns.insert(id, column);
    }

    let wires = topology
        .links
        .iter()
        .enumerate()
        .map(|(i, link)| {
            let (fx, fy) = cards[&link.from];
            let (tx, ty) = cards[&link.to];
            let mut points = vec![(fx + CARD_W, fy + CARD_H / 2)];
            // Through the channel the layout reserved, column by column.
            for &(column, centre) in layout.route(i) {
                points.push((column_x(column) + CARD_W / 2, ORIGIN.1 + centre - top));
            }
            points.push((tx, ty + CARD_H / 2));
            points
        })
        .collect();

    let (inner, straight) = layout.inner_segments();
    Drawing {
        cards,
        centres,
        columns,
        wires,
        stats: Stats {
            depth: layout.depth(),
            crossings: layout.crossings(),
            // Measured against the seed for BOTH modes: what a fresh pass cost
            // the viewer is only visible if the same question is asked of it.
            order_changes: layout.order_changes(&seed),
            inner,
            straight,
            bends: layout.bends(),
        },
    }
}

/// The service names in column `column`, top to bottom — the order the viewer
/// learns, and the one a stable relayout promises not to disturb.
fn column_order(topology: &Topology, drawing: &Drawing, column: usize) -> Vec<String> {
    let mut members: Vec<(i32, &str)> = drawing
        .columns
        .iter()
        .filter(|&(_, &c)| c == column)
        .map(|(id, _)| (drawing.centres[id], topology.name_of(*id)))
        .collect();
    members.sort_unstable();
    members
        .into_iter()
        .map(|(_, name)| name.to_string())
        .collect()
}

// --- Shared state -------------------------------------------------------------

/// The reactive holder the oracle mutates and the view reads. One instance,
/// shared by `Rc` — never two derived copies.
struct TopologyState {
    topology: Signal<Topology>,
    drawing: Signal<Drawing>,
    mode: Signal<Mode>,
    /// How far through [`STREAM`] the feed has played.
    cursor: Signal<usize>,
    /// What the last applied step was, for the status line.
    last_event: Signal<String>,
}

impl TopologyState {
    fn new() -> Self {
        let topology = seed_topology();
        let drawing = relayout(&topology, &BTreeMap::new(), Mode::Stable);
        Self {
            topology: Signal::new(topology),
            drawing: Signal::new(drawing),
            mode: Signal::new(Mode::Stable),
            cursor: Signal::new(0),
            last_event: Signal::new("9 dependencies discovered".to_string()),
        }
    }

    /// Apply `change` to the topology and re-place the graph.
    ///
    /// **Every mutation goes through here**, so a change can never land without
    /// the drawing being brought up to date — and the seed handed to the new
    /// pass is always exactly the drawing being replaced.
    fn apply(&self, note: &str, change: impl FnOnce(&mut Topology) -> bool) -> bool {
        let mut topology = self.topology.get();
        if !change(&mut topology) {
            return false;
        }
        let previous = self.drawing.get().centres;
        let drawing = relayout(&topology, &previous, self.mode.get());
        self.topology.set(topology);
        self.drawing.set(drawing);
        self.last_event.set(note.to_string());
        true
    }

    /// Re-place the graph without changing it — what switching mode does, and
    /// the cleanest way to see the three orderings differ on identical data.
    fn replace_drawing(&self, mode: Mode) {
        let previous = self.drawing.get().centres;
        let drawing = relayout(&self.topology.get(), &previous, mode);
        self.drawing.set(drawing);
    }

    /// **R1443 — tidy the drawing the viewer has, without adopting a new
    /// policy.** Re-place once with [`Mode::Settled`] and leave `mode` alone.
    ///
    /// This is the verb a stable view was missing: switching to `fresh` to
    /// relieve a tangle also changes what every LATER change will do, and
    /// switching back does not restore the drawing it discarded on the way. Here
    /// the mode signal is never written, so the next `advance` behaves exactly as
    /// it would have.
    ///
    /// Reports what it cost in the same two currencies the pass publishes.
    fn untangle(&self) -> String {
        let before = self.drawing.get().stats.crossings;
        self.replace_drawing(Mode::Settled);
        let after = self.drawing.get().stats;
        let note = format!(
            "untangled: {} -> {} crossings, {} pairs moved",
            before, after.crossings, after.order_changes
        );
        self.last_event.set(note.clone());
        note
    }

    /// Apply the next scripted step, or `None` at the end of the feed.
    fn advance(&self) -> Option<String> {
        let at = self.cursor.get();
        let step = *STREAM.get(at)?;
        let note = step.describe();
        let applied = match step {
            Step::Add(name) => self.apply(&note, |t| t.add(name).is_some()),
            Step::Remove(name) => self.apply(&note, |t| t.remove(name)),
            Step::Connect(from, to) => self.apply(&note, |t| t.connect(from, to)),
            Step::Disconnect(from, to) => self.apply(&note, |t| t.disconnect(from, to)),
        };
        self.cursor.set(at + 1);
        applied.then_some(note)
    }

    fn reset(&self) {
        let topology = seed_topology();
        let drawing = relayout(&topology, &BTreeMap::new(), self.mode.get());
        self.topology.set(topology);
        self.drawing.set(drawing);
        self.cursor.set(0);
        self.last_event.set("9 dependencies discovered".to_string());
    }
}

fn use_topology_state() -> Rc<TopologyState> {
    Owner::current()
        .expect("use_topology_state requires an active Owner scope")
        .cache(STATE_KEY, TopologyState::new)
}

// --- The oracle (primary External) --------------------------------------------

/// Publishes the topology, the placement, and what the last pass cost in both
/// currencies; drives the feed and the two orderings.
struct TopologyOracle {
    state: Option<Rc<TopologyState>>,
}

impl core::fmt::Debug for TopologyOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TopologyOracle")
            .field("attached", &self.state.is_some())
            .field("mode", &self.mode().name())
            .finish()
    }
}

impl TopologyOracle {
    fn new() -> Self {
        Self { state: None }
    }

    fn attach_state(&mut self, state: Rc<TopologyState>) {
        self.state = Some(state);
    }

    fn mode(&self) -> Mode {
        self.state.as_ref().map_or(Mode::Stable, |s| s.mode.get())
    }

    fn stats(&self) -> Stats {
        self.state
            .as_ref()
            .map_or_else(Stats::default, |s| s.drawing.get().stats)
    }

    /// A text argument, trimmed. A non-string is a
    /// [`TypeMismatch`](InvokeError::TypeMismatch): the same shape cannot
    /// succeed on a retry.
    fn text(arg: &IntrospectValue) -> Result<String, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => Ok(s.trim().to_string()),
            _ => Err(InvokeError::TypeMismatch),
        }
    }

    /// R1564 §5.15 (PINION-PR82) — the one sentence for "this external is not
    /// wired to a topology yet". Ten sites reached for it; a shared const keeps
    /// them one statement rather than ten that can drift.
    const NO_STATE: &str = "this topology surface is not bound to a model yet";

    /// A `"from,to"` pair.
    fn pair(arg: &IntrospectValue) -> Result<(String, String), InvokeError> {
        let raw = Self::text(arg)?;
        let (from, to) = raw.split_once(',').ok_or_else(|| {
            InvokeError::rejected(format!(
                "malformed argument {raw:?} (expected \"<from>,<to>\")"
            ))
        })?;
        Ok((from.trim().to_string(), to.trim().to_string()))
    }

    fn state(&mut self) -> Result<Rc<TopologyState>, InvokeError> {
        self.state
            .clone()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))
    }

    /// Resolve a service by name into its placed card, or reject.
    fn card(&self, name: &str) -> Result<(i32, i32), InvokeError> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))?;
        let id = state
            .topology
            .get()
            .id_of(name)
            .ok_or_else(|| InvokeError::rejected(format!("no service named {name:?}")))?;
        state.drawing.get().cards.get(&id).copied().ok_or_else(|| {
            InvokeError::rejected(format!(
                "service {name:?} exists but the current drawing places no card for it"
            ))
        })
    }
}

impl ExternalIntrospect for TopologyOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // Which ordering a relayout uses; the one writable path.
                    SchemaField::new("mode", "string"),
                    // The graph as it stands.
                    SchemaField::new("services", "int"),
                    SchemaField::new("dependencies", "int"),
                    SchemaField::new("depth", "int"),
                    SchemaField::new("service_names", "string"),
                    SchemaField::new("last_event", "string"),
                    SchemaField::new("feed_remaining", "int"),
                    // What the last pass cost, in both currencies.
                    SchemaField::new("crossings", "int"),
                    SchemaField::new("order_changes", "int"),
                    // The long-edge machinery, as data.
                    SchemaField::new("bends", "int"),
                    SchemaField::new("inner_segments", "int"),
                    SchemaField::new("straight_inner", "int"),
                    // Per-service placement, arg = the name.
                    SchemaField::new("node_x", "string"),
                    SchemaField::new("node_y", "string"),
                    SchemaField::new("node_column", "string"),
                    // The remembered order, arg = the column index.
                    SchemaField::new("column_order", "string"),
                    // A wire's polyline, arg = "from,to".
                    SchemaField::new("wire_points", "string"),
                    // The feed and the topology verbs.
                    SchemaField::new("advance", "string"),
                    SchemaField::new("reset", "string"),
                    // R1443 — tidy what is drawn now, leaving `mode` alone.
                    SchemaField::new("untangle", "string"),
                    SchemaField::new("add_service", "string"),
                    SchemaField::new("remove_service", "string"),
                    SchemaField::new("connect", "string"),
                    SchemaField::new("disconnect", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let measured = self.stats();
        let int = |v: usize| Some(IntrospectValue::Int(i64::try_from(v).unwrap_or(i64::MAX)));
        match path {
            "mode" => Some(IntrospectValue::Text(self.mode().name().to_string())),
            "services" => int(state.topology.get().services.len()),
            "dependencies" => int(state.topology.get().links.len()),
            "depth" => int(measured.depth),
            "service_names" => Some(IntrospectValue::Text(
                state
                    .topology
                    .get()
                    .services
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            "last_event" => Some(IntrospectValue::Text(state.last_event.get())),
            "feed_remaining" => int(STREAM.len().saturating_sub(state.cursor.get())),
            "crossings" => int(measured.crossings),
            "order_changes" => int(measured.order_changes),
            "bends" => int(measured.bends),
            "inner_segments" => int(measured.inner),
            "straight_inner" => int(measured.straight),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "mode" => {
                let IntrospectValue::Text(name) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let mode = Mode::parse(name.as_str()).ok_or(InterveneError::OutOfRange)?;
                if let Some(state) = self.state.as_ref() {
                    state.mode.set(mode);
                    // Re-place immediately: the mode is not a preference stored
                    // for next time, it is the ordering, and the viewer should
                    // see what it does to the drawing they are looking at.
                    state.replace_drawing(mode);
                }
                Ok(())
            }
            "services" | "dependencies" | "depth" | "service_names" | "last_event"
            | "feed_remaining" | "crossings" | "order_changes" | "bends" | "inner_segments"
            | "straight_inner" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "node_x" | "node_y" | "node_column" | "column_order" | "wire_points" => {
                self.read_placement(path, &args)
            }
            _ => self.run_verb(path, &args),
        }
    }
}

impl TopologyOracle {
    /// The placement oracles: reads of the CURRENT drawing that need an
    /// argument, which is what makes them `invoke` rather than `query`.
    fn read_placement(
        &self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "node_x" => Ok(IntrospectValue::Int(i64::from(
                self.card(&Self::text(args)?)?.0,
            ))),
            "node_y" => Ok(IntrospectValue::Int(i64::from(
                self.card(&Self::text(args)?)?.1,
            ))),
            "node_column" => {
                let name = Self::text(args)?;
                let state = self
                    .state
                    .as_ref()
                    .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))?;
                let id =
                    state.topology.get().id_of(&name).ok_or_else(|| {
                        InvokeError::rejected(format!("no service named {name:?}"))
                    })?;
                let column = state
                    .drawing
                    .get()
                    .columns
                    .get(&id)
                    .copied()
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "service {name:?} exists but the current drawing assigns it no column"
                        ))
                    })?;
                Ok(IntrospectValue::Int(
                    i64::try_from(column).unwrap_or(i64::MAX),
                ))
            }
            "column_order" => {
                let raw = Self::text(args)?;
                let column: usize = raw.parse().map_err(|_| {
                    InvokeError::rejected(format!("column_order: {raw:?} is not a column index"))
                })?;
                let state = self
                    .state
                    .as_ref()
                    .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))?;
                Ok(IntrospectValue::Text(
                    column_order(&state.topology.get(), &state.drawing.get(), column).join(","),
                ))
            }
            "wire_points" => {
                let (from, to) = Self::pair(args)?;
                let state = self
                    .state
                    .as_ref()
                    .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))?;
                let topology = state.topology.get();
                let (Some(from_id), Some(to_id)) = (topology.id_of(&from), topology.id_of(&to))
                else {
                    return Err(InvokeError::rejected(format!(
                        "wire_points: no service named {:?}",
                        if topology.id_of(&from).is_none() {
                            &from
                        } else {
                            &to
                        }
                    )));
                };
                let at = topology
                    .links
                    .iter()
                    .position(|l| l.from == from_id && l.to == to_id)
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "wire_points: {from} and {to} both exist but are not connected"
                        ))
                    })?;
                let drawing = state.drawing.get();
                let points = drawing.wires.get(at).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "wire_points: link {from} -> {to} is in the topology \
                         but the current drawing routed no wire for it"
                    ))
                })?;
                Ok(IntrospectValue::Text(
                    points
                        .iter()
                        .map(|&(x, y)| format!("{x},{y}"))
                        .collect::<Vec<_>>()
                        .join(";"),
                ))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }

    /// The topology verbs: everything that CHANGES the graph, and therefore
    /// re-places it through `TopologyState::apply`.
    fn run_verb(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "advance" => {
                let state = self.state()?;
                state.advance().map(IntrospectValue::Text).ok_or_else(|| {
                    InvokeError::rejected("advance: the scripted timeline has no further step")
                })
            }
            "reset" => {
                self.state()?.reset();
                Ok(IntrospectValue::Text("reset".to_string()))
            }
            "untangle" => Ok(IntrospectValue::Text(self.state()?.untangle())),
            "add_service" => {
                let name = Self::text(args)?;
                let note = format!("+ {name} appeared");
                let state = self.state()?;
                state
                    .apply(&note, |t| t.add(&name).is_some())
                    .then(|| IntrospectValue::Text(note.clone()))
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "add: a service named {name:?} is already in the topology"
                        ))
                    })
            }
            "remove_service" => {
                let name = Self::text(args)?;
                let note = format!("- {name} went away");
                let state = self.state()?;
                state
                    .apply(&note, |t| t.remove(&name))
                    .then(|| IntrospectValue::Text(note.clone()))
                    .ok_or_else(|| {
                        InvokeError::rejected(format!("remove: no service named {name:?}"))
                    })
            }
            "connect" => {
                let (from, to) = Self::pair(args)?;
                let note = format!("+ {from} -> {to}");
                let state = self.state()?;
                state
                    .apply(&note, |t| t.connect(&from, &to))
                    .then(|| IntrospectValue::Text(note.clone()))
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "connect: {from} -> {to} names a service that is not \
                             in the topology, or a link that is already there"
                        ))
                    })
            }
            "disconnect" => {
                let (from, to) = Self::pair(args)?;
                let note = format!("- {from} -> {to}");
                let state = self.state()?;
                state
                    .apply(&note, |t| t.disconnect(&from, &to))
                    .then(|| IntrospectValue::Text(note.clone()))
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "disconnect: no link {from} -> {to} in the topology"
                        ))
                    })
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl External for TopologyOracle {
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

/// Clamp a signed view coordinate into the unsigned pixel space a `Rect` uses.
fn upx(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

/// A path point from integer view coordinates, which are far inside f32's
/// exact-integer range.
#[allow(
    clippy::cast_precision_loss,
    reason = "view coordinates are < 2^13, exactly representable in f32"
)]
fn ppt(x: i32, y: i32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

/// One wire as a stroked polyline through the points the layout chose.
///
/// The commands are rebased on the path's own rect (R1358), and the rect is the
/// bounding box of every point — including the bends, which is the whole reason
/// a long wire's bbox is honest about the columns it crosses.
fn wire_scene(points: &[(i32, i32)], stroke: Stroke, tag: String) -> Option<Scene> {
    let (first, tail) = points.split_first()?;
    let xs: Vec<i32> = points.iter().map(|p| p.0).collect();
    let ys: Vec<i32> = points.iter().map(|p| p.1).collect();
    let ox = xs.iter().copied().min()?;
    let oy = ys.iter().copied().min()?;
    let bw = (xs.iter().copied().max()? - ox).max(1);
    let bh = (ys.iter().copied().max()? - oy).max(1);
    let rect = Rect::new(upx(ox), upx(oy), upx(bw), upx(bh));
    // Rebase by the RECT's origin, not the raw minimum: `upx` clamps a negative
    // minimum to 0, and the paint adapter translates by exactly `rect.{x,y}`.
    let (org_x, org_y) = (
        i32::try_from(rect.x).unwrap_or(0),
        i32::try_from(rect.y).unwrap_or(0),
    );
    let mut commands = vec![PathCommand::MoveTo(ppt(first.0 - org_x, first.1 - org_y))];
    commands.extend(
        tail.iter()
            .map(|&(x, y)| PathCommand::LineTo(ppt(x - org_x, y - org_y))),
    );
    Some(Scene::Path(
        PathNode::new(rect, commands, PathStyle::stroked(stroke))
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h))
                    .with_pointer_transparent(true),
            ),
    ))
}

/// One service card: a filled, outlined box with the service name in it.
fn card_scene(name: &str, at: (i32, i32), fill: Color, ink: Color, outline: Color) -> Vec<Scene> {
    let rect = Rect::new(upx(at.0), upx(at.1), upx(CARD_W), upx(CARD_H));
    vec![
        Scene::Box(
            BoxNode::new(
                rect,
                BoxStyle::filled(fill).with_border(Border::new(outline, 1)),
            )
            .with_tag(format!("topology.node.{name}"))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h)),
            ),
        ),
        Scene::Text(
            TextNode::styled(
                name,
                Rect::default(),
                TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(ink),
            )
            .with_tag(format!("topology.label.{name}"))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x + 12, rect.y + 10)
                    .with_size(Size::px(CARD_W as u32 - 16, LABEL_FONT_PX + 6)),
            ),
        ),
    ]
}

/// The status line: what the ordering just cost, in both currencies.
///
/// The ordering is named by [`Mode::name`], the same string the introspect
/// surface publishes — a second mapping here would be free to disagree with what
/// an agent reads back, which is the drift a view like this exists to rule out.
fn status_text(mode: Mode, stats: Stats, event: &str) -> String {
    format!(
        "{event} — {} relayout: {} crossing(s), {} remembered pair(s) reordered, \
         {}/{} inner segment(s) straight",
        mode.name(),
        stats.crossings,
        stats.order_changes,
        stats.straight,
        stats.inner
    )
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "view-fn shape mirrors the WidgetCore::view(&Frame) trait signature"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_topology_state();
    let topology = state.topology.get();
    let drawing = state.drawing.get();

    let mut children = vec![Scene::Text(TextNode::styled(
        "Service topology — placed by pinion-graph, re-placed on every change",
        Rect::new(
            upx(ORIGIN.0),
            22,
            WIN_W - upx(ORIGIN.0) * 2,
            TITLE_FONT_PX + 4,
        ),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ))];
    children.push(Scene::Text(TextNode::styled(
        status_text(state.mode.get(), drawing.stats, &state.last_event.get()),
        Rect::new(
            upx(ORIGIN.0),
            52,
            WIN_W - upx(ORIGIN.0) * 2,
            STATUS_FONT_PX + 4,
        ),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    )));

    // Wires first, so a card always paints over the line that reaches it.
    let wire = Stroke::new(theme.resolve(ColorRole::Outline), 2).with_cap(StrokeCap::Round);
    for (link, points) in topology.links.iter().zip(&drawing.wires) {
        let tag = format!(
            "topology.wire.{}-{}",
            topology.name_of(link.from),
            topology.name_of(link.to)
        );
        if let Some(scene) = wire_scene(points, wire, tag) {
            children.push(scene);
        }
    }

    let fill = theme.resolve(ColorRole::SurfaceContainerHigh);
    let ink = theme.resolve(ColorRole::OnSurface);
    let outline = theme.resolve(ColorRole::Outline);
    for service in &topology.services {
        if let Some(&at) = drawing.cards.get(&service.id) {
            children.extend(card_scene(&service.name, at, fill, ink, outline));
        }
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct TopologyView;

impl WidgetCore for TopologyView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = TopologyOracle::new();
        oracle.attach_state(use_topology_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-topology (R1442 §5.38 §5.52)"
    }
}

impl WidgetA11y for TopologyView {
    /// The view is a WAI-ARIA `img` whose value text names the graph AND what
    /// the last pass cost. An AT user who cannot see the drawing is exactly the
    /// user for whom "did anything move?" is otherwise unanswerable.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_topology_state();
        let topology = state.topology.get();
        let drawing = state.drawing.get();
        let columns: BTreeSet<usize> = drawing.columns.values().copied().collect();
        let described = columns
            .iter()
            .map(|&column| {
                format!(
                    "column {column}: {}",
                    column_order(&topology, &drawing, column).join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Service topology")
                .with_value(AccessValue::Text(format!(
                    "{} services in {} columns, {} dependencies, {} ordering; {}",
                    topology.services.len(),
                    drawing.stats.depth,
                    topology.links.len(),
                    state.mode.get().name(),
                    described
                ))),
        ]
    }
}

impl WidgetView for TopologyView {
    type Renderer = HelloTopologyRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TopologyView>();
}

#[cfg(test)]
mod tests;
