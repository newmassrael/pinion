// R838 §5.38 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, NodeGraphExternal, bezier, …).
#![allow(clippy::doc_markdown)]

//! `hello-node-editor` — R838 §5.38 §5.40 §5.51 **node-graph editor
//! substrate**: a canvas of titled nodes, each with input / output ports,
//! connected by cubic-bezier edges. This is the visual-scripting /
//! material-graph form factor and the first seed of the self-hosted
//! editor's blueprint / shader-graph panel (Phase B value-order #1 — the
//! highest-leverage advanced DCC widget after the R836 property grid and
//! R837 data grid).
//!
//! ## Pure composition of existing substrate (use-substrate, not invent)
//!
//! The node editor introduces **no new framework primitive** — it composes:
//!
//! * **Node move** — the R51.34 capture lock
//!   ([`External::wants_pointer_capture`] +
//!   [`External::pointer_move`]), normalised against the *canvas* rect (the
//!   stable pixel reference, exactly as R786 column-resize normalises against
//!   the grid viewport, [[capture-drag-stable-pixel-reference]]). A live drag
//!   and the AI-first `intervene node.<i>.x` path are one source of truth.
//! * **Edge connect** — the R742 drag substrate
//!   ([`External::begin_drag`] from an output port →
//!   [`External::drag_release`] over an input port). `drag_release`'s `over`
//!   carries the dropped-on tag, so the target input port falls straight out
//!   of the router's hit-test — no per-binding drop resolver.
//! * **Edge paint** — [`Scene::Path`] cubic beziers (the R721 vector-path
//!   substrate; commands are absolute device pixels untouched by the flex
//!   pass, so a single window coordinate space aligns nodes and wires).
//! * **Free node placement** — [`LayoutStyle::with_absolute_position`].
//! * **Reactive model** — [`Owner::cache`] Signals shared by the view and the
//!   coordinator ([[reactive-holder-for-shared-external-view-state]]).
//!
//! ## Architecture — one coordinator External
//!
//! [`NodeGraphExternal`] (`node_graph`, the primary + the single keyboard Tab
//! stop) owns the whole graph: the `Signal<Vec<GraphNode>>` node model, the
//! `Signal<Vec<Edge>>` connection list, a single [`Selection`] sum type (node |
//! edge | none — `Signal<Selection>`), and a live-drag preview. Nodes and edges
//! carry stable [`NodeId`] / [`EdgeId`] handles (R841): addressing is by id, so
//! deleting one entity never renumbers the survivors. It exposes the graph for
//! AI-first introspection: `query node_count` / `edge_count` / `node_ids` /
//! `edge_ids` / `node.<id>.{title,x,y,inputs,outputs}` / `edge.<id>` /
//! `selected` / `selected_edge`; `intervene node.<id>.x` / `node.<id>.y` /
//! `selected` / `selected_edge`; `invoke add_edge` / `remove_edge` /
//! `delete_node` / `delete_selected` / `nudge` / the pointer `send` wire.
//!
//! ## Keyboard (single Tab stop, the graph)
//!
//! Arrow keys nudge the selected node; `Delete` / `Backspace` removes the
//! selection (node + its incident edges, or an edge); `Escape` clears it.
//!
//! ## a11y (R838 §5.40, R840 fix)
//!
//! The graph lowers to a WAI-ARIA `group` of `generic` children — one per node,
//! named `"<title> (<n> in, <m> out)"`, the selected node flagged. It is NOT a
//! `list` (a graph is not an ordered set — that would assert a false
//! `aria-posinset`). A true diagram / `graphics-document` role with a
//! multi-target `flowto` relation (so edges/connectivity are perceivable to AT)
//! needs a `pinion-a11y` role that does not exist yet — the topology stays
//! reachable on the RPC axis ([[ai-first-rpc-introspection-obligation]]).
//!
//! ## Known gaps (honest carry — what this seed deliberately does NOT do yet)
//!
//! - **Scale**: the model is a `Vec` with O(n) id lookups + a clone per
//!   `Signal::get()`. Fine for this demo; a real editor graph lifts to a
//!   `SlotMap`/`HashMap`-by-id store (stable ids make that swap interface-
//!   transparent — R841 was the prerequisite).
//! - **Typed ports**: ports are input/output *counts*, not typed sockets, so
//!   `add_edge` validates arity but not type (a Float output can wire to any
//!   input). The type lattice differs per graph kind (material vs blueprint),
//!   so it is deferred until that consumer settles it ([[abstraction-needs-second-consumer]]).
//! - **Persistence / undo**: the serde derives are load-bearing for `Signal`
//!   storage, but no save/load or undo command-history is wired yet.
//! - **`add_node` over RPC**: the graph can be mutated / connected / deleted but
//!   not grown (no node-creation verb); edge-id minting is in place for it.
//! - **Crate extraction**: the model + pure bezier geometry are example-local;
//!   the 2nd consumer (a self-hosted material graph) lifts them to a crate
//!   (the chip / stepper inline-first precedent) — extract, not extend.
//! - Node rename (inline editor), canvas pan / zoom, multi-select marquee — all
//!   additive follow-ups.

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use pinion_a11y::{
    toolbar_button_nodes, AccessNode, AccessState, AriaRole, ToolbarControl, WidgetA11y,
};
use pinion_core::composite_tag::{split_send_payload, split_subindex};
use pinion_core::external::{
    int_of, Backend, BackendFallback, BackendSupport, CaptureNormalize, DragPayload, DropPoint,
    External, ExternalIntrospect, IntrospectSchema, IntrospectValue, InterveneError, InvokeError,
    RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, PathStyle, Size,
    Stroke, StrokeCap, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloNodeEditorRenderer, HelloNodeEditorRendererError);

// ─── window + canvas constants ─────────────────────────────────────

const WIN_W: u32 = 640;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// Primary External — the graph coordinator (the single keyboard Tab stop)
/// and the canvas hit / capture-normalize root.
const GRAPH_TAG: &str = "node_graph";

/// R849 — the "add node" palette: the node kinds a sidebar click (or the
/// `add_node` RPC verb) can create, as `(title, inputs, outputs)`. A tiny
/// material-graph vocabulary (sources / op / sink), the same port shapes
/// [`default_nodes`] seeds.
const PALETTE: &[(&str, usize, usize)] = &[
    ("Texture", 0, 1),
    ("Color", 0, 1),
    ("Multiply", 2, 1),
    ("Add", 2, 1),
    ("Output", 1, 0),
];

/// R849 — sidebar width for the node palette. The canvas keeps its
/// `WIN_W × WIN_H` coordinate system; the palette is an extra left strip, so
/// the capture-drag / hit-test canvas-extent math is unchanged (the
/// `capture_normalize` reference is the offset `GRAPH_TAG` rect).
const PALETTE_W: u32 = 132;
/// R849 — the palette container tag (a11y `toolbar` root; routes no pointer
/// events itself — only its `node_graph#palette_<idx>` item cards do).
const PALETTE_TAG: &str = "node_palette";
/// R849 — the editor a11y root wrapping the palette + the canvas.
const ROOT_TAG: &str = "node_editor";

/// R849 — where a newly added node first lands, and the per-add cascade step
/// (in minted-id order) so repeated adds do not stack exactly.
const SPAWN_X: i32 = 300;
const SPAWN_Y: i32 = 44;
const SPAWN_STEP: i32 = 26;

const TITLE_PX: u32 = 20;
const NODE_TITLE_PX: u32 = 14;
const STATUS_PX: u32 = 12;

// Node-card geometry (logical px, i32 so the coordinate arithmetic never
// casts). Converted to `u32` only at the `Size` / `absolute_position` seam.
const NODE_W: i32 = 130;
const HEADER_H: i32 = 30;
const PORT_PITCH: i32 = 28;
const PORT_SIZE: i32 = 12;
const BODY_PAD: i32 = 10;
const MIN_NODE_Y: i32 = 44;

const EDGE_W: u32 = 3;
const SELECTED_EDGE_W: u32 = 5;
const PREVIEW_W: u32 = 2;
const NUDGE_STEP: i32 = 12;

/// Click-to-select tolerance for a wire (window px) + the bezier sample count.
const EDGE_HIT_THRESHOLD: i32 = 8;
const EDGE_SAMPLES: i32 = 18;
/// Minimum horizontal control-point offset for a wire (so near-vertical wires
/// still bow into a readable S-curve rather than a straight diagonal).
const MIN_WIRE_BOW: i32 = 60;

// ─── numeric conversion helpers (centralise the few unavoidable casts) ──

/// `i32` → `u32` for a `Size` / `absolute_position` argument; negatives floor
/// at `0` (positions are clamped non-negative before they reach here).
fn upx(v: i32) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(0)
}

/// `usize` index → `i32` for port-row arithmetic (port counts are tiny).
fn irow(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(0)
}

/// Round a drag-math `f64` pixel value to `i32`, mirroring the R786
/// `px_to_width` clamp-then-round idiom. The clamp to the `i32` range
/// proves the cast cannot truncate, so the lint is a false positive here.
#[allow(clippy::cast_possible_truncation)]
fn round_i32(px: f64) -> i32 {
    px.clamp(f64::from(i32::MIN), f64::from(i32::MAX)).round() as i32
}

/// A path point from integer window coordinates. Coordinates are small
/// (< 2^13), well inside f32's exact-integer range, so the precision-loss
/// lint does not apply.
#[allow(clippy::cast_precision_loss)]
fn ppt(x: i32, y: i32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

// ─── graph model (example-local; lifted at the 2nd consumer) ───────

/// Stable node handle (R841). Minted once at creation, never reused, never
/// shifted — so an edge / selection / (future) undo record that references a
/// node survives the deletion of *other* nodes. Positional indices (the R838
/// model) invalidated every reference on each delete; this is the
/// `hello-dock-panels-editor` stable-id discipline applied to the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct NodeId(u32);

/// Stable edge handle (R841), same discipline as [`NodeId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct EdgeId(u32);

impl NodeId {
    fn raw(self) -> u32 {
        self.0
    }
}

impl EdgeId {
    fn raw(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for NodeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::fmt::Display for EdgeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One node: a titled card with `inputs` input ports (left edge) and
/// `outputs` output ports (right edge), placed at canvas `(x, y)`. Carries a
/// stable [`NodeId`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct GraphNode {
    id: NodeId,
    title: String,
    x: i32,
    y: i32,
    inputs: usize,
    outputs: usize,
}

impl GraphNode {
    fn new(id: u32, title: &str, x: i32, y: i32, inputs: usize, outputs: usize) -> Self {
        Self { id: NodeId(id), title: title.to_owned(), x, y, inputs, outputs }
    }

    /// Port rows = the taller of the two columns (at least one, so a
    /// source / sink node still has a body).
    fn rows(&self) -> i32 {
        irow(self.inputs.max(self.outputs).max(1))
    }

    fn height(&self) -> i32 {
        HEADER_H + self.rows() * PORT_PITCH + BODY_PAD
    }
}

/// A directed connection (output port → input port), addressed by stable
/// [`NodeId`]s and carrying its own stable [`EdgeId`]. Deleting a node drops
/// only its incident edges — no other edge's identity changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Edge {
    id: EdgeId,
    from_node: NodeId,
    from_port: usize,
    to_node: NodeId,
    to_port: usize,
}

/// Live drag preview while a wire is being pulled from an output port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Preview {
    from_node: NodeId,
    from_port: usize,
    /// Currently-hovered input port the wire would snap to, if any.
    to: Option<(NodeId, usize)>,
}

/// What is selected — a node, an edge, or nothing. A sum type so "both a node
/// and an edge selected" is unrepresentable; the handles are stable ids, so a
/// selection survives an unrelated delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Selection {
    None,
    Node(NodeId),
    Edge(EdgeId),
}

impl Selection {
    /// The selected node id, if a node is selected.
    fn node(self) -> Option<NodeId> {
        match self {
            Selection::Node(id) => Some(id),
            _ => None,
        }
    }

    /// The selected edge id, if an edge is selected.
    fn edge(self) -> Option<EdgeId> {
        match self {
            Selection::Edge(id) => Some(id),
            _ => None,
        }
    }
}

/// The id new edges mint from: one past the highest [`default_edges`] id.
/// Derived (not a hand-maintained const) so it can never drift out of sync
/// with the defaults — adding a seed edge cannot silently collide a minted id.
fn first_dynamic_edge_id() -> u32 {
    default_edges().iter().map(|e| e.id.raw()).max().map_or(0, |m| m + 1)
}

/// R849 — the id new nodes mint from: one past the highest [`default_nodes`]
/// id. Derived (mirroring [`first_dynamic_edge_id`]) so adding a seed node
/// cannot silently collide a minted id.
fn first_dynamic_node_id() -> u32 {
    default_nodes().iter().map(|n| n.id.raw()).max().map_or(0, |m| m + 1)
}

/// First-paint graph — a tiny material graph (`Texture` × `Color` →
/// `Multiply` → `Output`). Node ids 0..=3, edge ids 0..=2.
fn default_nodes() -> Vec<GraphNode> {
    vec![
        GraphNode::new(0, "Texture", 40, 70, 0, 1),
        GraphNode::new(1, "Color", 40, 210, 0, 1),
        GraphNode::new(2, "Multiply", 250, 110, 2, 1),
        GraphNode::new(3, "Output", 470, 150, 1, 0),
    ]
}

fn default_edges() -> Vec<Edge> {
    vec![
        Edge { id: EdgeId(0), from_node: NodeId(0), from_port: 0, to_node: NodeId(2), to_port: 0 },
        Edge { id: EdgeId(1), from_node: NodeId(1), from_port: 0, to_node: NodeId(2), to_port: 1 },
        Edge { id: EdgeId(2), from_node: NodeId(2), from_port: 0, to_node: NodeId(3), to_port: 0 },
    ]
}

// ─── geometry (window coordinates; canvas == window) ───────────────

/// The vertical offset of port row `row` within a node card (top of the
/// port box).
fn port_row_top(row: usize) -> i32 {
    HEADER_H + irow(row) * PORT_PITCH + PORT_PITCH / 2 - PORT_SIZE / 2
}

/// Centre of input port `i` of `node`, in window coordinates.
fn input_port_center(node: &GraphNode, i: usize) -> (i32, i32) {
    (node.x + PORT_SIZE / 2, node.y + port_row_top(i) + PORT_SIZE / 2)
}

/// Centre of output port `j` of `node`, in window coordinates.
fn output_port_center(node: &GraphNode, j: usize) -> (i32, i32) {
    (node.x + NODE_W - PORT_SIZE / 2, node.y + port_row_top(j) + PORT_SIZE / 2)
}

/// Cubic-bezier control points for a wire from output `from` to input `to`.
/// The single SSOT both the edge paint ([`view_edge`]) and the edge
/// hit-test ([`point_near_edge`]) read, so the drawn curve and the clickable
/// curve can never diverge ([[two-text-layouts-paint-vs-geometry]] analogue).
fn edge_curve(from: (i32, i32), to: (i32, i32)) -> ((i32, i32), (i32, i32)) {
    let ctrl = (to.0 - from.0).abs().max(MIN_WIRE_BOW) / 2;
    ((from.0 + ctrl, from.1), (to.0 - ctrl, to.1))
}

/// A point on the cubic bezier at parameter `t` (Bernstein form). `f64::from`
/// is lossless for the small integer coordinates, so no precision cast.
fn cubic_at(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    t: f64,
) -> (f64, f64) {
    let mt = 1.0 - t;
    let w0 = mt * mt * mt;
    let w1 = 3.0 * mt * mt * t;
    let w2 = 3.0 * mt * t * t;
    let w3 = t * t * t;
    (
        w0 * p0.0 + w1 * p1.0 + w2 * p2.0 + w3 * p3.0,
        w0 * p0.1 + w1 * p1.1 + w2 * p2.1 + w3 * p3.1,
    )
}

/// Squared distance from `point` to segment `seg_a`–`seg_b`.
fn point_seg_dist2(point: (f64, f64), seg_a: (f64, f64), seg_b: (f64, f64)) -> f64 {
    let (vx, vy) = (seg_b.0 - seg_a.0, seg_b.1 - seg_a.1);
    let (wx, wy) = (point.0 - seg_a.0, point.1 - seg_a.1);
    let len2 = vx * vx + vy * vy;
    let t = if len2 <= 0.0 { 0.0 } else { ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (seg_a.0 + t * vx, seg_a.1 + t * vy);
    let (dx, dy) = (point.0 - cx, point.1 - cy);
    dx * dx + dy * dy
}

/// Whether `(px, py)` (window px) lands within [`EDGE_HIT_THRESHOLD`] of the
/// wire from `from` to `to` — the click-to-select-an-edge predicate. The
/// curve is sampled into [`EDGE_SAMPLES`] segments and the click tested
/// against each (a thin wire needs no analytic root-finding).
fn point_near_edge(px: i32, py: i32, from: (i32, i32), to: (i32, i32)) -> bool {
    let (c1, c2) = edge_curve(from, to);
    let click = (f64::from(px), f64::from(py));
    let p0 = (f64::from(from.0), f64::from(from.1));
    let p1 = (f64::from(c1.0), f64::from(c1.1));
    let p2 = (f64::from(c2.0), f64::from(c2.1));
    let p3 = (f64::from(to.0), f64::from(to.1));
    let thr2 = f64::from(EDGE_HIT_THRESHOLD) * f64::from(EDGE_HIT_THRESHOLD);
    let mut prev = p0;
    for step in 1..=EDGE_SAMPLES {
        let t = f64::from(step) / f64::from(EDGE_SAMPLES);
        let cur = cubic_at(p0, p1, p2, p3, t);
        if point_seg_dist2(click, prev, cur) <= thr2 {
            return true;
        }
        prev = cur;
    }
    false
}

fn clamp_node_x(x: i32) -> i32 {
    let max = i32::try_from(WIN_W).unwrap_or(i32::MAX) - NODE_W;
    x.clamp(0, max.max(0))
}

fn clamp_node_y(y: i32) -> i32 {
    let max = i32::try_from(WIN_H).unwrap_or(i32::MAX) - HEADER_H - 8;
    y.clamp(MIN_NODE_Y, max.max(MIN_NODE_Y))
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

#[must_use]
fn use_nodes() -> Rc<Signal<Vec<GraphNode>>> {
    let owner = Owner::current().expect("use_nodes requires an active Owner scope");
    owner.cache("node_graph.nodes", || Signal::new(default_nodes()))
}

#[must_use]
fn use_edges() -> Rc<Signal<Vec<Edge>>> {
    let owner = Owner::current().expect("use_edges requires an active Owner scope");
    owner.cache("node_graph.edges", || Signal::new(default_edges()))
}

#[must_use]
fn use_selection() -> Rc<Signal<Selection>> {
    let owner = Owner::current().expect("use_selection requires an active Owner scope");
    owner.cache("node_graph.selection", || Signal::new(Selection::None))
}

#[must_use]
fn use_preview() -> Rc<Signal<Option<Preview>>> {
    let owner = Owner::current().expect("use_preview requires an active Owner scope");
    owner.cache("node_graph.preview", || Signal::new(None))
}

/// Monotonic [`EdgeId`] allocator — persists across view-fn re-runs so a
/// minted id is never reused (the stable-identity guarantee for new edges).
#[must_use]
fn use_next_edge_id() -> Rc<Cell<u32>> {
    let owner = Owner::current().expect("use_next_edge_id requires an active Owner scope");
    owner.cache("node_graph.next_edge_id", || Cell::new(first_dynamic_edge_id()))
}

/// R849 — the monotonic [`NodeId`] source for newly created nodes (mirrors
/// [`use_next_edge_id`]). One shared `Cell` per Owner scope so a minted id is
/// never reused, even across deletes.
fn use_next_node_id() -> Rc<Cell<u32>> {
    let owner = Owner::current().expect("use_next_node_id requires an active Owner scope");
    owner.cache("node_graph.next_node_id", || Cell::new(first_dynamic_node_id()))
}

// ─── sub-tag grammar (composite paint tags route to the coordinator) ──

/// `node_graph#node_<id>` → [`NodeId`].
fn parse_node_sub(sub: &str) -> Option<NodeId> {
    Some(NodeId(sub.strip_prefix("node_")?.parse().ok()?))
}

/// R849 — `node_graph#palette_<idx>` → the [`PALETTE`] index of an add-node
/// sidebar card.
fn parse_palette_sub(sub: &str) -> Option<usize> {
    sub.strip_prefix("palette_")?.parse().ok()
}

/// `oport_<id>_<j>` → (node id, output port).
fn parse_oport_sub(sub: &str) -> Option<(NodeId, usize)> {
    let (n, j) = sub.strip_prefix("oport_")?.split_once('_')?;
    Some((NodeId(n.parse().ok()?), j.parse().ok()?))
}

/// A full drop tag `node_graph#iport_<id>_<i>` → (node id, input port). Uses
/// the canonical `#` splitter (`split_subindex`) rather than an inline split.
fn parse_input_port_tag(tag: &str) -> Option<(NodeId, usize)> {
    let (n, i) = split_subindex(tag).1?.strip_prefix("iport_")?.split_once('_')?;
    Some((NodeId(n.parse().ok()?), i.parse().ok()?))
}

/// Two comma-separated `i32`s ("dx,dy").
fn parse_pair_i32(s: &str) -> Option<(i32, i32)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Join stable ids into the CSV the `node_ids` / `edge_ids` queries return.
fn csv_ids(ids: impl Iterator<Item = u32>) -> String {
    ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

/// Four comma-separated values: "from_node_id,from_port,to_node_id,to_port".
fn parse_quad(csv: &str) -> Option<(NodeId, usize, NodeId, usize)> {
    let parts: Vec<&str> = csv.split(',').collect();
    let [fnode, fport, tnode, tport] = parts.as_slice() else {
        return None;
    };
    Some((
        NodeId(fnode.trim().parse().ok()?),
        fport.trim().parse().ok()?,
        NodeId(tnode.trim().parse().ok()?),
        tport.trim().parse().ok()?,
    ))
}

// ─── internal drag latches (Cell — not read by the view) ───────────

/// Snapshot taken on the first capture move of a node drag (the R786
/// `ResizeDragStart` analogue at 2-D).
#[derive(Clone, Copy, Debug)]
struct NodeDragStart {
    press_x_rel: f64,
    press_y_rel: f64,
    x_at_press: i32,
    y_at_press: i32,
}

/// What the most recent `PointerDown` landed on — read by `begin_drag` (an
/// output-port press arms an edge drag) and `pointer_move` (a node-body press
/// arms a move).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingPress {
    /// No press in flight, or a press on the bare canvas background (the
    /// edge-click-select probe path — distinct from `InputPort` so an input
    /// port press is not mistaken for an empty-canvas click).
    None,
    NodeBody,
    OutputPort(NodeId, usize),
    InputPort,
    /// R850 — a press on an add-node palette card. Like [`InputPort`](Self::InputPort)
    /// it is a non-drag press that must suppress the background edge-probe, but
    /// it is recorded as its own variant rather than borrowing `InputPort`'s
    /// name so a future input-port-specific branch can never misread a palette
    /// press as a real input-port press.
    Palette,
}

// ─── coordinator External ──────────────────────────────────────────

/// The node-graph coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) plus the internal drag latches.
struct NodeGraphExternal {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    edges: Rc<Signal<Vec<Edge>>>,
    /// The single selection (node | edge | none) — a sum type over stable ids,
    /// so node/edge selection is mutually exclusive AND survives an unrelated
    /// delete (a dangling selection is pruned by [`Self::validate_selection`]).
    selection: Rc<Signal<Selection>>,
    preview: Rc<Signal<Option<Preview>>>,
    /// Monotonic [`EdgeId`] source for newly-connected wires.
    next_edge_id: Rc<Cell<u32>>,
    /// R849 — monotonic [`NodeId`] source for newly created (palette / RPC) nodes.
    next_node_id: Rc<Cell<u32>>,
    /// Node grabbed for a capture-drag move (set on a node-body `PointerDown`).
    grabbed_node: Cell<Option<NodeId>>,
    node_drag: Cell<Option<NodeDragStart>>,
    pending_press: Cell<PendingPress>,
    /// Edge under the most recent background press (the capture-seed
    /// `pointer_move` records it; a background `PointerUp` consumes it).
    pending_edge_hit: Cell<Option<EdgeId>>,
}

impl NodeGraphExternal {
    fn new(
        nodes: Rc<Signal<Vec<GraphNode>>>,
        edges: Rc<Signal<Vec<Edge>>>,
        selection: Rc<Signal<Selection>>,
        preview: Rc<Signal<Option<Preview>>>,
        next_edge_id: Rc<Cell<u32>>,
        next_node_id: Rc<Cell<u32>>,
    ) -> Self {
        Self {
            nodes,
            edges,
            selection,
            preview,
            next_edge_id,
            next_node_id,
            grabbed_node: Cell::new(None),
            node_drag: Cell::new(None),
            pending_press: Cell::new(PendingPress::None),
            pending_edge_hit: Cell::new(None),
        }
    }

    fn node_count(&self) -> usize {
        self.nodes.get().len()
    }

    fn node_by_id(&self, id: NodeId) -> Option<GraphNode> {
        self.nodes.get().into_iter().find(|n| n.id == id)
    }

    /// Clear the selection if it references a node / edge that no longer exists.
    /// With stable ids this replaces all of R838's index-shift bookkeeping: a
    /// structural mutation cannot *renumber* a survivor, only *remove* one, so
    /// "is the selection still alive?" is the entire adjustment.
    fn validate_selection(&self) {
        let alive = match self.selection.get() {
            Selection::None => true,
            Selection::Node(id) => self.nodes.get().iter().any(|n| n.id == id),
            Selection::Edge(id) => self.edges.get().iter().any(|e| e.id == id),
        };
        if !alive {
            self.selection.set(Selection::None);
        }
    }

    /// Move node `id` to a clamped `(x, y)`. The single mutation behind both
    /// the capture drag and the `intervene node.<id>.{x,y}` path.
    fn set_node_pos(&self, id: NodeId, x: i32, y: i32) -> bool {
        let mut moved = false;
        self.nodes.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(node) = next.iter_mut().find(|n| n.id == id) {
                node.x = clamp_node_x(x);
                node.y = clamp_node_y(y);
                moved = true;
            }
            next
        });
        moved
    }

    /// R849 — create a new node of [`PALETTE`] kind `kind` at the next cascade
    /// position, minting a fresh stable [`NodeId`] (monotonic, never reused),
    /// and select it. Returns the new id, or `None` for an out-of-range kind.
    /// The single mutation behind both a palette card click ([`handle_send`])
    /// and the `add_node` RPC verb — the graph can finally *grow*, not only be
    /// rearranged. A new node has no edges, so no edge / selection bookkeeping
    /// is needed (the stable-id model: adding is purely additive).
    fn add_node(&self, kind: usize) -> Option<NodeId> {
        let &(title, inputs, outputs) = PALETTE.get(kind)?;
        let raw = self.next_node_id.get();
        self.next_node_id.set(raw + 1);
        let id = NodeId(raw);
        // Cascade in minted order from the spawn point so repeated adds fan out
        // instead of stacking exactly on one another.
        let step = i32::try_from(raw.saturating_sub(first_dynamic_node_id())).unwrap_or(0) % 8;
        let x = clamp_node_x(SPAWN_X + step * SPAWN_STEP);
        let y = clamp_node_y(SPAWN_Y + step * SPAWN_STEP);
        self.nodes.set_with(|prev| {
            let mut next = prev.clone();
            next.push(GraphNode { id, title: title.to_owned(), x, y, inputs, outputs });
            next
        });
        self.selection.set(Selection::Node(id));
        Some(id)
    }

    /// Add an edge output `(from_node, from_port)` → input `(to_node,
    /// to_port)`. Rejects a self-loop, a missing node, an out-of-range port, or
    /// a duplicate; an input port takes a single wire, so an existing
    /// connection into the target input is replaced (the canonical node-editor
    /// rule). The new edge mints a fresh stable [`EdgeId`].
    fn add_edge(&self, from_node: NodeId, from_port: usize, to_node: NodeId, to_port: usize) -> bool {
        if from_node == to_node {
            return false;
        }
        let nodes = self.nodes.get();
        let Some(src) = nodes.iter().find(|n| n.id == from_node) else {
            return false;
        };
        let Some(dst) = nodes.iter().find(|n| n.id == to_node) else {
            return false;
        };
        if from_port >= src.outputs || to_port >= dst.inputs {
            return false;
        }
        let mut edges = self.edges.get();
        let dup = edges.iter().any(|e| {
            e.from_node == from_node
                && e.from_port == from_port
                && e.to_node == to_node
                && e.to_port == to_port
        });
        if dup {
            return false;
        }
        // Input single-wire rule: drop any existing wire into the target input.
        edges.retain(|e| !(e.to_node == to_node && e.to_port == to_port));
        let id = EdgeId(self.next_edge_id.get());
        self.next_edge_id.set(id.raw() + 1);
        edges.push(Edge { id, from_node, from_port, to_node, to_port });
        self.edges.set(edges);
        // The dedup-retain may have removed the selected edge — prune if gone.
        self.validate_selection();
        true
    }

    /// Remove the edge with stable id `id` (no-op + `false` if absent).
    fn remove_edge(&self, id: EdgeId) -> bool {
        let mut edges = self.edges.get();
        let before = edges.len();
        edges.retain(|e| e.id != id);
        if edges.len() == before {
            return false;
        }
        self.edges.set(edges);
        self.validate_selection();
        true
    }

    /// Delete node `id` and its incident edges. No reindex: every surviving
    /// node and edge keeps its stable id, so references elsewhere stay valid.
    fn delete_node(&self, id: NodeId) -> bool {
        if !self.nodes.get().iter().any(|n| n.id == id) {
            return false;
        }
        self.nodes.set_with(|prev| prev.iter().filter(|n| n.id != id).cloned().collect());
        self.edges
            .set_with(|prev| prev.iter().copied().filter(|e| e.from_node != id && e.to_node != id).collect());
        self.validate_selection();
        self.grabbed_node.set(None);
        self.node_drag.set(None);
        true
    }

    /// Delete whatever is selected — node or edge (the single `Selection` makes
    /// the two cases exhaustive). The `Delete` key + RPC `delete_selected` share it.
    fn delete_selected(&self) -> bool {
        match self.selection.get() {
            Selection::Edge(e) => self.remove_edge(e),
            Selection::Node(k) => self.delete_node(k),
            Selection::None => false,
        }
    }

    /// Hit-test a window-px click against every wire; the first within
    /// tolerance is the selection candidate (its stable [`EdgeId`]).
    fn hit_test_edge(&self, px: i32, py: i32) -> Option<EdgeId> {
        let nodes = self.nodes.get();
        self.edges
            .get()
            .iter()
            .find(|e| {
                let (Some(src), Some(dst)) = (
                    nodes.iter().find(|n| n.id == e.from_node),
                    nodes.iter().find(|n| n.id == e.to_node),
                ) else {
                    return false;
                };
                point_near_edge(
                    px,
                    py,
                    output_port_center(src, e.from_port),
                    input_port_center(dst, e.to_port),
                )
            })
            .map(|e| e.id)
    }

    /// Nudge the selected node by `(dx, dy)` (the arrow-key path).
    fn nudge_selected(&self, dx: i32, dy: i32) -> bool {
        let Some(id) = self.selection.get().node() else {
            return false;
        };
        let Some(node) = self.node_by_id(id) else {
            return false;
        };
        self.set_node_pos(id, node.x + dx, node.y + dy)
    }

    /// Select a node by id (must exist). The sum type makes any prior edge
    /// selection vanish for free — no "clear the other" bookkeeping.
    fn select_node(&self, id: Option<NodeId>) {
        let next = id
            .filter(|id| self.nodes.get().iter().any(|n| n.id == *id))
            .map_or(Selection::None, Selection::Node);
        self.selection.set(next);
    }

    /// Select an edge by id (must exist).
    fn select_edge(&self, id: Option<EdgeId>) {
        let next = id
            .filter(|id| self.edges.get().iter().any(|e| e.id == *id))
            .map_or(Selection::None, Selection::Edge);
        self.selection.set(next);
    }

    /// Pointer `send` wire (the same channel the router and RPC share).
    fn handle_send(&mut self, payload: &str) -> IntrospectValue {
        // Decode via the canonical send-wire SSOT (`split_send_payload`):
        // a composite `"key:event[:mods]"` yields `(Some(key), event)`; a bare
        // `"event"` (canvas background) yields `(None, event)`.
        let (sub, event) = match split_send_payload(payload) {
            Some((key, event, _mods)) => (Some(key), event),
            None => (None, payload),
        };
        match (sub, event) {
            (Some(s), "PointerDown") => {
                if parse_node_sub(s).is_some() {
                    self.grabbed_node.set(parse_node_sub(s));
                    self.node_drag.set(None);
                    self.pending_press.set(PendingPress::NodeBody);
                } else if let Some((n, j)) = parse_oport_sub(s) {
                    self.pending_press.set(PendingPress::OutputPort(n, j));
                } else if parse_palette_sub(s).is_some() {
                    // R850 — a palette card press: not a drag, not an input
                    // port. Recorded as its own variant so the background
                    // edge-probe is suppressed without lying about what was
                    // pressed (the activation runs on the matching PointerUp).
                    self.pending_press.set(PendingPress::Palette);
                } else {
                    // An input-port press — distinct from a background press so
                    // it never triggers the edge-click probe.
                    self.pending_press.set(PendingPress::InputPort);
                }
            }
            (Some(s), "PointerUp") => {
                // R849 — a palette card's release creates a node (the activation
                // edge); a node card's release selects it. (A palette press set
                // PendingPress::Palette above, suppressing the edge-probe; the
                // gesture is reset by `end_gesture` after this branch.)
                if let Some(kind) = parse_palette_sub(s) {
                    self.add_node(kind);
                } else if let Some(n) = parse_node_sub(s) {
                    self.select_node(Some(n));
                }
                self.end_gesture();
            }
            (None, "PointerUp") => {
                // Background release: select the edge the capture-seed press
                // probe landed on, else deselect everything. Only when no node
                // drag was armed.
                if self.grabbed_node.get().is_none() {
                    match self.pending_edge_hit.get() {
                        Some(e) => self.select_edge(Some(e)),
                        None => self.select_node(None),
                    }
                }
                self.end_gesture();
            }
            (None, "PointerDown") => {
                self.pending_press.set(PendingPress::None);
                self.pending_edge_hit.set(None);
            }
            _ => {}
        }
        IntrospectValue::Null
    }

    fn end_gesture(&self) {
        self.grabbed_node.set(None);
        self.node_drag.set(None);
        self.pending_press.set(PendingPress::None);
        self.pending_edge_hit.set(None);
    }
}

impl core::fmt::Debug for NodeGraphExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NodeGraphExternal")
            .field("nodes", &self.node_count())
            .field("edges", &self.edges.get().len())
            .field("selection", &self.selection.get())
            .finish_non_exhaustive()
    }
}

impl External for NodeGraphExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Tui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt into the capture lock so a node-body drag survives the cursor
    /// straying off the card (the canonical drag-to-move UX).
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Normalise the captured cursor against the whole canvas (the primary
    /// `node_graph` rect = the window). The canvas does not move when a node
    /// does, so it is the stable pixel reference (R786 column-resize idiom).
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Tag(GRAPH_TAG)
    }

    /// Capture-drag a node. `x_rel` / `y_rel` are the cursor's fraction across
    /// the canvas; the first move snapshots the press anchor, each later move
    /// applies `pos_at_press + (rel − press_rel) · canvas_extent`.
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let Some(node) = self.grabbed_node.get() else {
            // Not dragging a node. A background press (the R51.35 capture seed
            // forwards the press cursor here) probes for an edge under the
            // click so a background `PointerUp` can select it. An input-port
            // press is excluded via `PendingPress::InputPort`.
            if self.pending_press.get() == PendingPress::None {
                let px = round_i32(f64::from(x_rel) * f64::from(WIN_W));
                let py = round_i32(f64::from(y_rel) * f64::from(WIN_H));
                self.pending_edge_hit.set(self.hit_test_edge(px, py));
            }
            return;
        };
        match self.node_drag.get() {
            None => {
                if let Some(n) = self.node_by_id(node) {
                    self.node_drag.set(Some(NodeDragStart {
                        press_x_rel: f64::from(x_rel),
                        press_y_rel: f64::from(y_rel),
                        x_at_press: n.x,
                        y_at_press: n.y,
                    }));
                }
            }
            Some(start) => {
                let dx = round_i32((f64::from(x_rel) - start.press_x_rel) * f64::from(WIN_W));
                let dy = round_i32((f64::from(y_rel) - start.press_y_rel) * f64::from(WIN_H));
                self.set_node_pos(node, start.x_at_press + dx, start.y_at_press + dy);
            }
        }
    }

    /// Arm an edge drag from an output port (the R742 drag substrate). The
    /// payload carries the source `(node, port)`; `None` for any other press,
    /// so a node-body press falls through to the capture-drag move path.
    fn begin_drag(&self) -> Option<DragPayload> {
        if let PendingPress::OutputPort(n, j) = self.pending_press.get() {
            self.preview.set(Some(Preview { from_node: n, from_port: j, to: None }));
            return Some(DragPayload {
                kind: Cow::Borrowed("node-edge"),
                value: IntrospectValue::Text(format!("{n}_{j}")),
            });
        }
        None
    }

    /// Live preview: snap the dragged wire's loose end to the hovered input
    /// port (if any) so the connection target reads from the router hit-test.
    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        let target = over.and_then(|dp| parse_input_port_tag(&dp.tag));
        self.preview.set_with(|prev| (*prev).map(|p| Preview { to: target, ..p }));
    }

    /// Commit the edge if the drop landed on an input port.
    fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        if let (IntrospectValue::Text(src), Some((to_node, to_port))) =
            (&payload.value, over.as_ref().and_then(|dp| parse_input_port_tag(&dp.tag)))
        {
            if let Some((from_node, from_port)) = src
                .split_once('_')
                .and_then(|(a, b)| Some((NodeId(a.parse().ok()?), b.parse::<usize>().ok()?)))
            {
                self.add_edge(from_node, from_port, to_node, to_port);
            }
        }
        self.preview.set(None);
        self.end_gesture();
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for NodeGraphExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("node_count", "int"),
            ("edge_count", "int"),
            ("node_ids", "string"),
            ("edge_ids", "string"),
            ("selected", "int"),
            ("selected_edge", "int"),
            ("node.<id>.title", "string"),
            ("node.<id>.x", "int"),
            ("node.<id>.y", "int"),
            ("node.<id>.inputs", "int"),
            ("node.<id>.outputs", "int"),
            ("edge.<id>", "string"),
            ("send", "string"),
            ("add_node", "string"),
            ("add_edge", "string"),
            ("remove_edge", "int"),
            ("delete_node", "int"),
            ("delete_selected", "json"),
            ("nudge", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "node_count" => Some(IntrospectValue::Int(int_of(self.node_count()))),
            "edge_count" => Some(IntrospectValue::Int(int_of(self.edges.get().len()))),
            // CSV of the *current* (possibly sparse after deletes) stable ids —
            // the enumeration handle an AI needs now that addressing is by id.
            "node_ids" => Some(IntrospectValue::Text(csv_ids(
                self.nodes.get().iter().map(|n| n.id.raw()),
            ))),
            "edge_ids" => Some(IntrospectValue::Text(csv_ids(
                self.edges.get().iter().map(|e| e.id.raw()),
            ))),
            "selected" => Some(match self.selection.get().node() {
                Some(id) => IntrospectValue::Int(i64::from(id.raw())),
                None => IntrospectValue::Null,
            }),
            "selected_edge" => Some(match self.selection.get().edge() {
                Some(id) => IntrospectValue::Int(i64::from(id.raw())),
                None => IntrospectValue::Null,
            }),
            _ => {
                if let Some(rest) = path.strip_prefix("node.") {
                    let (id_str, field) = rest.split_once('.')?;
                    let id = NodeId(id_str.parse().ok()?);
                    let nodes = self.nodes.get();
                    let node = nodes.iter().find(|n| n.id == id)?;
                    return match field {
                        "title" => Some(IntrospectValue::Text(node.title.clone())),
                        "x" => Some(IntrospectValue::Int(i64::from(node.x))),
                        "y" => Some(IntrospectValue::Int(i64::from(node.y))),
                        "inputs" => Some(IntrospectValue::Int(int_of(node.inputs))),
                        "outputs" => Some(IntrospectValue::Int(int_of(node.outputs))),
                        _ => None,
                    };
                }
                if let Some(id_str) = path.strip_prefix("edge.") {
                    let id = EdgeId(id_str.parse().ok()?);
                    let edges = self.edges.get();
                    let e = edges.iter().find(|e| e.id == id)?;
                    return Some(IntrospectValue::Text(format!(
                        "{}:{}->{}:{}",
                        e.from_node, e.from_port, e.to_node, e.to_port
                    )));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        if path == "selected" {
            return match value {
                IntrospectValue::Null => {
                    self.select_node(None);
                    Ok(())
                }
                IntrospectValue::Int(i) => {
                    let id = u32::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.select_node(Some(NodeId(id)));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            };
        }
        if path == "selected_edge" {
            return match value {
                IntrospectValue::Null => {
                    self.select_edge(None);
                    Ok(())
                }
                IntrospectValue::Int(i) => {
                    let id = u32::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.select_edge(Some(EdgeId(id)));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            };
        }
        let Some(rest) = path.strip_prefix("node.") else {
            return Err(InterveneError::UnknownPath);
        };
        let (id_str, field) = rest.split_once('.').ok_or(InterveneError::UnknownPath)?;
        let id = NodeId(id_str.parse().map_err(|_| InterveneError::UnknownPath)?);
        let Some(node) = self.node_by_id(id) else {
            return Err(InterveneError::UnknownPath);
        };
        // The field decides read-only-ness first (a read-only field rejects
        // any value type), then `x` / `y` require an `Int`.
        match field {
            "x" | "y" => {
                let IntrospectValue::Int(v) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let coord = i32::try_from(v).map_err(|_| InterveneError::TypeMismatch)?;
                if field == "x" {
                    self.set_node_pos(id, coord, node.y);
                } else {
                    self.set_node_pos(id, node.x, coord);
                }
                Ok(())
            }
            "title" | "inputs" | "outputs" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(s) => Ok(self.handle_send(&s)),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R849 — create a node by kind name; returns the new node's stable
            // id in one round-trip. An unknown kind is Rejected (the graph is
            // unchanged), the AI-first mirror of a clicked palette card.
            "add_node" => match args {
                IntrospectValue::Text(s) => {
                    let kind = PALETTE
                        .iter()
                        .position(|&(name, _, _)| name == s)
                        .ok_or(InvokeError::Rejected)?;
                    let id = self.add_node(kind).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Int(i64::from(id.raw())))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "add_edge" => match args {
                IntrospectValue::Text(s) => {
                    let (fnode, fport, tnode, tport) =
                        parse_quad(&s).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.add_edge(fnode, fport, tnode, tport)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "remove_edge" => match args {
                IntrospectValue::Int(i) => {
                    let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.remove_edge(EdgeId(id))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "delete_node" => match args {
                IntrospectValue::Int(i) => {
                    let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.delete_node(NodeId(id))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "delete_selected" => Ok(IntrospectValue::Bool(self.delete_selected())),
            "nudge" => match args {
                IntrospectValue::Text(s) => {
                    let (dx, dy) = parse_pair_i32(&s).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.nudge_selected(dx, dy)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ─── keyboard (graph focused) ──────────────────────────────────────

fn nudge_ok(intro: &mut dyn ExternalIntrospect, dx: i32, dy: i32) -> bool {
    matches!(
        intro.invoke("nudge", IntrospectValue::Text(format!("{dx},{dy}"))),
        Ok(IntrospectValue::Bool(true))
    )
}

fn apply_key_graph(scene: &mut Scene, key: &str) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(GRAPH_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    match key {
        "ArrowUp" => nudge_ok(intro, 0, -NUDGE_STEP),
        "ArrowDown" => nudge_ok(intro, 0, NUDGE_STEP),
        "ArrowLeft" => nudge_ok(intro, -NUDGE_STEP, 0),
        "ArrowRight" => nudge_ok(intro, NUDGE_STEP, 0),
        "Delete" | "Backspace" => matches!(
            intro.invoke("delete_selected", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        ),
        "Escape" => {
            let _ = intro.intervene("selected", IntrospectValue::Null);
            true
        }
        _ => false,
    }
}

// ─── paint ─────────────────────────────────────────────────────────

/// One cubic-bezier edge path between two window-space port centres. An
/// S-curve: control points offset horizontally so wires leave / enter ports
/// level (the canonical node-graph wire shape).
fn view_edge(tag: String, from: (i32, i32), to: (i32, i32), color: Color, width: u32) -> Scene {
    let (c1, c2) = edge_curve(from, to);
    let commands = vec![
        PathCommand::MoveTo(ppt(from.0, from.1)),
        PathCommand::CurveTo { c1: ppt(c1.0, c1.1), c2: ppt(c2.0, c2.1), end: ppt(to.0, to.1) },
    ];
    // Bounding box over ALL four control points (the curve bows outside the
    // endpoint box, so a snapshot bbox from endpoints alone understates the
    // true extent — an AI-first `scene/snapshot` bbox query must be honest).
    // `pointer_transparent` keeps the decorative edge from intercepting a
    // click meant for the canvas background or a node card beneath it.
    let xs = [from.0, c1.0, c2.0, to.0];
    let ys = [from.1, c1.1, c2.1, to.1];
    let ox = xs.iter().copied().min().unwrap_or(0);
    let oy = ys.iter().copied().min().unwrap_or(0);
    let bw = (xs.iter().copied().max().unwrap_or(0) - ox).max(1);
    let bh = (ys.iter().copied().max().unwrap_or(0) - oy).max(1);
    Scene::Path(
        PathNode::new(
            Rect::new(upx(ox), upx(oy), upx(bw), upx(bh)),
            commands,
            PathStyle::stroked(Stroke::new(color, width).with_cap(StrokeCap::Round)),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(upx(ox), upx(oy))
                .with_size(Size::px(upx(bw), upx(bh)))
                .with_pointer_transparent(true),
        ),
    )
}

/// Look up a node by its stable id within a slice (the view's id→node resolve).
fn node_ref(nodes: &[GraphNode], id: NodeId) -> Option<&GraphNode> {
    nodes.iter().find(|n| n.id == id)
}

/// All committed edges, resolved to their port centres. Painted behind the
/// node cards; the selected edge paints thicker in the highlight colour. Each
/// edge is tagged by its stable [`EdgeId`].
fn view_edges(
    nodes: &[GraphNode],
    edges: &[Edge],
    selected_edge: Option<EdgeId>,
    theme: &Theme,
) -> Vec<Scene> {
    let color = theme.resolve(ColorRole::Accent);
    let hot = theme.resolve(ColorRole::OnSurface);
    edges
        .iter()
        .filter_map(|e| {
            let from = output_port_center(node_ref(nodes, e.from_node)?, e.from_port);
            let to = input_port_center(node_ref(nodes, e.to_node)?, e.to_port);
            let (c, w) = if selected_edge == Some(e.id) {
                (hot, SELECTED_EDGE_W)
            } else {
                (color, EDGE_W)
            };
            Some(view_edge(format!("{GRAPH_TAG}#edge_{}", e.id), from, to, c, w))
        })
        .collect()
}

/// One port box (a small rounded square on the card edge).
fn view_port(tag: String, left: i32, top: i32, color: Color) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag)
            .with_style(BoxStyle::filled(color).with_corner_radius(upx(PORT_SIZE / 2)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(left), upx(top))
                    .with_size(Size::px(upx(PORT_SIZE), upx(PORT_SIZE))),
            ),
    )
}

/// One node card: a header (title) over its input (left) + output (right)
/// ports, absolutely placed at the node's canvas position. The whole card is
/// one drag target; the ports are deeper hit targets for edge connect.
fn view_node(node: &GraphNode, selected: bool, theme: &Theme) -> Scene {
    let id = node.id;
    let port_color = theme.resolve(ColorRole::Accent);
    let header = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            node.title.clone(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(NODE_TITLE_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(0, 0)
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(upx(NODE_W), upx(HEADER_H))),
        ),
    );

    let mut children = vec![header];
    for i in 0..node.inputs {
        children.push(view_port(
            format!("{GRAPH_TAG}#iport_{id}_{i}"),
            0,
            port_row_top(i),
            port_color,
        ));
    }
    for j in 0..node.outputs {
        children.push(view_port(
            format!("{GRAPH_TAG}#oport_{id}_{j}"),
            NODE_W - PORT_SIZE,
            port_row_top(j),
            port_color,
        ));
    }

    let border = if selected {
        Border::new(theme.resolve(ColorRole::Accent), 2)
    } else {
        Border::new(theme.resolve(ColorRole::Outline), 1)
    };
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRAPH_TAG}#node_{id}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(border))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(node.x), upx(node.y))
                    .with_size(Size::px(upx(NODE_W), upx(node.height()))),
            ),
    )
}

/// R849 — the "add node" palette sidebar: a labelled column of clickable cards,
/// one per [`PALETTE`] kind. Each card is a `node_graph#palette_<idx>` composite,
/// so a click routes to the coordinator's `handle_send` (the same wire the
/// `add_node` RPC verb drives) and creates that node. Plain clickable cards (the
/// grouped-header precedent — a list of actions), not Material buttons;
/// press-state feedback is an additive axis.
fn view_palette(theme: &Theme) -> Scene {
    let mut items: Vec<Scene> = Vec::with_capacity(PALETTE.len() + 1);
    items.push(Scene::Text(
        TextNode::styled(
            "Add node",
            Rect::default(),
            TextStyle::new().with_size_px(NODE_TITLE_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_padding(Rect::new(12, 12, 12, 4))),
    ));
    for (idx, &(title, inputs, outputs)) in PALETTE.iter().enumerate() {
        let label = Scene::Text(TextNode::styled(
            format!("{title} ({inputs}/{outputs})"),
            Rect::default(),
            TextStyle::new().with_size_px(13).with_fg(theme.resolve(ColorRole::OnSurface)),
        ));
        items.push(Scene::Container(
            ContainerNode::new(vec![label])
                .with_tag(format!("{GRAPH_TAG}#palette_{idx}"))
                .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(PALETTE_W - 16, 30))
                        .with_padding(Rect::new(10, 0, 8, 0)),
                ),
        ));
    }
    Scene::Container(
        ContainerNode::new(items)
            .with_tag(PALETTE_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(6)
                    .with_size(Size::px(PALETTE_W, WIN_H)),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let nodes = use_nodes().get();
    let edges = use_edges().get();
    let selection = use_selection().get();
    let selected = selection.node();
    let selected_edge = selection.edge();
    let preview = use_preview().get();

    let mut children: Vec<Scene> = Vec::new();

    // Edges (behind) → preview wire → node cards (on top) → chrome.
    children.extend(view_edges(&nodes, &edges, selected_edge, &theme));

    if let Some(p) = preview {
        if let Some(from_node) = node_ref(&nodes, p.from_node) {
            let from = output_port_center(from_node, p.from_port);
            let to = p
                .to
                .and_then(|(tn, tp)| Some(input_port_center(node_ref(&nodes, tn)?, tp)))
                .unwrap_or(from);
            children.push(view_edge(
                format!("{GRAPH_TAG}#preview"),
                from,
                to,
                theme.resolve(ColorRole::OnSurfaceMuted),
                PREVIEW_W,
            ));
        }
    }

    for node in &nodes {
        children.push(view_node(node, selected == Some(node.id), &theme));
    }

    children.push(Scene::Text(
        TextNode::styled(
            "Node graph",
            Rect::default(),
            TextStyle::new().with_size_px(TITLE_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, 12)),
    ));

    let sel_label = if let Some(id) = selected {
        format!("node {}", node_ref(&nodes, id).map_or("—", |n| n.title.as_str()))
    } else if let Some(e) = selected_edge {
        format!("edge {e}")
    } else {
        "none".to_owned()
    };
    let status = format!("{} nodes · {} edges · selected: {sel_label}", nodes.len(), edges.len());
    children.push(Scene::Text(
        TextNode::styled(
            status,
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, i32::try_from(WIN_H).map_or(0, |h| upx(h - 26)))),
    ));

    let canvas = Scene::Container(
        ContainerNode::new(children)
            .with_tag(GRAPH_TAG)
            .with_aria_label("Node graph")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    );
    // R849 — palette sidebar beside the canvas. The canvas keeps its
    // `WIN_W × WIN_H` coordinate system (its rect is merely offset by the
    // palette width; `capture_normalize` resolves against that offset rect), so
    // none of the node geometry / drag math changes.
    Scene::Container(
        ContainerNode::new(vec![view_palette(&theme), canvas]).with_layout(
            LayoutStyle::new().flex(FlexDirection::Row).with_size(Size::px(PALETTE_W + WIN_W, WIN_H)),
        ),
    )
}

// ─── WidgetCore impl ───────────────────────────────────────────────

struct NodeEditorView;

impl WidgetCore for NodeEditorView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(NodeGraphExternal::new(
            use_nodes(),
            use_edges(),
            use_selection(),
            use_preview(),
            use_next_edge_id(),
            use_next_node_id(),
        ))
    }

    fn tag() -> &'static str {
        GRAPH_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-node-editor (R838 §5.38 node-graph editor)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The canvas is the single keyboard tab stop. R850 — the add-node palette
    /// is **not** yet a tab stop: it is mouse/RPC-driven (the `add_node` verb
    /// and `scene/click` reach it), and its a11y `toolbar` is emitted with
    /// `focused_control: None` (the `hello-textarea` NoFocus-toolbar shape).
    /// Keyboard roving over the palette (a second tab stop with arrow/Enter) is
    /// a documented carry, not a silent gap.
    fn focusable_tags() -> Vec<&'static str> {
        vec![GRAPH_TAG]
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: Modifiers,
    ) -> bool {
        match focused {
            Some(GRAPH_TAG) => apply_key_graph(scene, key),
            _ => false,
        }
    }

    fn update(_state: (), _intent: &pinion_core::Intent) -> Vec<Command> {
        Vec::new()
    }
}

impl WidgetA11y for NodeEditorView {
    /// R838 §5.40 / R840 — the graph lowers to a WAI-ARIA `group` whose
    /// children are the nodes (named by title + port arity, selection flagged).
    ///
    /// R840 audit fix: a node graph is **not** a linear list, so it must not
    /// use `list`/`listitem` + `aria-posinset`/`aria-setsize` (which would
    /// assert a false "item i of n in an ordered set" to assistive tech). It
    /// lowers to `group` + neutral `generic` children — honest about being an
    /// unordered set of nodes. The connectivity (edges) is still invisible to
    /// AT: WAI-ARIA's `graphics-document` role and a multi-target
    /// `flowto`/`owns` relation are a genuine `pinion-a11y` gap (no graphics
    /// role or fan-out relation exists today) — the topology stays reachable
    /// on the RPC axis (`query edge.<i>`) per [[ai-first-rpc-introspection-obligation]].
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        let nodes = use_nodes().get();
        let selected = use_selection().get().node();
        // R849/R850 — the editor lowers to a root with two regions: the add-node
        // palette (a `toolbar` of `button`s that create nodes) and the graph
        // canvas (the R840 unordered `group` of node `generic`s).
        let root = AccessNode::new(ROOT_TAG, AriaRole::Group)
            .with_name("Node editor")
            .with_child(PALETTE_TAG)
            .with_child(GRAPH_TAG);
        // R850 — the palette `toolbar` + `button`s come from the
        // `toolbar_button_nodes` SSOT (gaining aria-posinset/setsize), not a
        // hand-rolled equivalent. `focused_control: None` because the palette is
        // not yet a keyboard tab stop (`focusable_tags` is canvas-only) — a
        // mouse/RPC-driven toolbar, the `hello-textarea` NoFocus-toolbar
        // precedent. Keyboard roving over the palette is a documented carry.
        let palette_tags: Vec<String> =
            (0..PALETTE.len()).map(|i| format!("{GRAPH_TAG}#palette_{i}")).collect();
        let palette_names: Vec<String> = PALETTE.iter().map(|&(t, _, _)| format!("Add {t}")).collect();
        let controls: Vec<ToolbarControl> = palette_tags
            .iter()
            .zip(&palette_names)
            .map(|(tag, name)| ToolbarControl { tag: tag.as_str(), name: Some(name.as_str()), checked: None })
            .collect();
        let mut out = vec![root];
        out.extend(toolbar_button_nodes(PALETTE_TAG, "Add node", &controls, None));
        let mut group = AccessNode::new(GRAPH_TAG, AriaRole::Group)
            .with_name("Node graph")
            .with_state(AccessState { focused: focused == Some(GRAPH_TAG), ..AccessState::default() });
        for node in &nodes {
            group = group.with_child(format!("{GRAPH_TAG}#node_{}", node.id));
        }
        out.push(group);
        for node in &nodes {
            out.push(
                AccessNode::new(format!("{GRAPH_TAG}#node_{}", node.id), AriaRole::Generic)
                    .with_name(format!("{} ({} in, {} out)", node.title, node.inputs, node.outputs))
                    .with_selected(selected == Some(node.id)),
            );
        }
        out
    }
}

impl WidgetView for NodeEditorView {
    type Renderer = HelloNodeEditorRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: PALETTE_W + WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<NodeEditorView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn boot_scene() -> Scene {
        Scene::External(ExternalNode::new(NodeEditorView::create_external()).with_tag(GRAPH_TAG))
    }

    fn graph_intro(scene: &Scene) -> &dyn ExternalIntrospect {
        scene
            .find_external_with_tag(GRAPH_TAG)
            .and_then(|n| n.handle.introspect())
            .expect("graph external present")
    }

    fn query_int(scene: &Scene, path: &str) -> i64 {
        match graph_intro(scene).query(path) {
            Some(IntrospectValue::Int(v)) => v,
            other => panic!("expected Int at {path}, got {other:?}"),
        }
    }

    /// Send a pointer wire event through the coordinator (a borrow-scoped
    /// helper so the `&mut scene` borrow ends before the next read).
    fn send(scene: &mut Scene, wire: &str) {
        let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let _ = intro.invoke("send", IntrospectValue::Text(wire.to_owned()));
    }

    #[test]
    fn r838_shape_and_defaults() {
        assert_eq!(default_nodes().len(), 4);
        assert_eq!(default_edges().len(), 3);
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = graph_intro(&scene);
            assert_eq!(intro.query("node_count"), Some(IntrospectValue::Int(4)));
            assert_eq!(intro.query("edge_count"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("selected"), Some(IntrospectValue::Null));
            assert_eq!(intro.query("node.2.title"), Some(IntrospectValue::Text("Multiply".to_owned())));
            assert_eq!(intro.query("node.2.inputs"), Some(IntrospectValue::Int(2)));
            assert_eq!(intro.query("node.3.outputs"), Some(IntrospectValue::Int(0)));
            assert_eq!(intro.query("edge.0"), Some(IntrospectValue::Text("0:0->2:0".to_owned())));
            assert_eq!(intro.query("node.9.title"), None, "out-of-range -> None");
        });
    }

    #[test]
    fn r838_intervene_moves_node_clamped() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("node.0.x", IntrospectValue::Int(120)).is_ok());
            assert!(intro.intervene("node.0.y", IntrospectValue::Int(90)).is_ok());
            assert_eq!(intro.query("node.0.x"), Some(IntrospectValue::Int(120)));
            assert_eq!(intro.query("node.0.y"), Some(IntrospectValue::Int(90)));
            // Off-canvas request clamps to the window bounds.
            assert!(intro.intervene("node.0.x", IntrospectValue::Int(99999)).is_ok());
            let x = intro.query("node.0.x");
            assert_eq!(x, Some(IntrospectValue::Int(i64::from(clamp_node_x(99999)))));
        });
    }

    #[test]
    fn r838_intervene_readonly_and_typed() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.intervene("node.0.title", IntrospectValue::Text("x".to_owned())),
                Err(InterveneError::ReadOnly),
            );
            assert_eq!(
                intro.intervene("node.0.x", IntrospectValue::Text("no".to_owned())),
                Err(InterveneError::TypeMismatch),
            );
            assert_eq!(
                intro.intervene("node.9.x", IntrospectValue::Int(0)),
                Err(InterveneError::UnknownPath),
            );
        });
    }

    #[test]
    fn r838_add_edge_validates_and_dedups_input() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Self-loop rejected.
            assert_eq!(
                intro.invoke("add_edge", IntrospectValue::Text("2,0,2,0".to_owned())),
                Ok(IntrospectValue::Bool(false)),
            );
            // Out-of-range port rejected.
            assert_eq!(
                intro.invoke("add_edge", IntrospectValue::Text("0,5,3,0".to_owned())),
                Ok(IntrospectValue::Bool(false)),
            );
            // A new valid edge into Output's only input replaces edge id 2's
            // target; the new wire mints a fresh id (3), edge id 2 is gone.
            assert_eq!(
                intro.invoke("add_edge", IntrospectValue::Text("0,0,3,0".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            // Input (3,0) now has exactly one wire — still 3 edges total.
            assert_eq!(intro.query("edge_count"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("edge.2"), None, "old wire id 2 was replaced");
            assert_eq!(intro.query("edge.3"), Some(IntrospectValue::Text("0:0->3:0".to_owned())));
        });
    }

    #[test]
    fn r838_remove_edge_by_id() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("remove_edge", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("edge_count"), Some(IntrospectValue::Int(2)));
            assert_eq!(
                intro.invoke("remove_edge", IntrospectValue::Int(9)),
                Ok(IntrospectValue::Bool(false)),
            );
        });
    }

    #[test]
    fn r841_delete_node_keeps_stable_ids_over_rpc() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("selected", IntrospectValue::Int(3)); // Output
            // Delete node id 1 (Color): drops only edge 1:0->2:1. NO reindex.
            assert_eq!(
                intro.invoke("delete_node", IntrospectValue::Int(1)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("node_count"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("edge_count"), Some(IntrospectValue::Int(2)));
            // Multiply is STILL id 2; edge id 0 still reads 0:0->2:0 (not renumbered).
            assert_eq!(intro.query("node.2.title"), Some(IntrospectValue::Text("Multiply".to_owned())));
            assert_eq!(intro.query("node.1.title"), None, "id 1 is gone, not reused");
            assert_eq!(intro.query("edge.0"), Some(IntrospectValue::Text("0:0->2:0".to_owned())));
            // Selection (Output id 3) is untouched — it did not shift to 2.
            assert_eq!(intro.query("selected"), Some(IntrospectValue::Int(3)));
        });
    }

    #[test]
    fn r838_send_selects_node_on_release() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("send", IntrospectValue::Text("node_2:PointerDown".to_owned()));
            let _ = intro.invoke("send", IntrospectValue::Text("node_2:PointerUp".to_owned()));
            assert_eq!(intro.query("selected"), Some(IntrospectValue::Int(2)));
            // Background release deselects.
            let _ = intro.invoke("send", IntrospectValue::Text("PointerUp".to_owned()));
            assert_eq!(intro.query("selected"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r838_capture_drag_moves_grabbed_node() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Press node 0's body, then capture-move across the canvas.
            send(&mut scene, "node_0:PointerDown");
            let x0 = query_int(&scene, "node.0.x");
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                node.handle.pointer_move(0.10, 0.10); // anchor
                node.handle.pointer_move(0.30, 0.10); // +0.20 * WIN_W to the right
            }
            let x1 = query_int(&scene, "node.0.x");
            assert!(x1 > x0, "node moved right under the capture drag ({x0} -> {x1})");
        });
    }

    #[test]
    fn r838_port_drag_creates_edge() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Remove the existing wire into Multiply input 1, then re-make it
            // by dragging from Color's output port onto it.
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("remove_edge", IntrospectValue::Int(1));
            }
            assert_eq!(query_int(&scene, "edge_count"), 2);
            // Press Color's output port (node 1, port 0) → begin_drag arms.
            send(&mut scene, "oport_1_0:PointerDown");
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let payload = node.handle.begin_drag().expect("output-port press arms a drag");
                assert_eq!(payload.kind.as_ref(), "node-edge");
                let drop =
                    DropPoint { tag: format!("{GRAPH_TAG}#iport_2_1"), x_rel: 0.5, y_rel: 0.5 };
                node.handle.drag_release(&payload, Some(drop));
            }
            assert_eq!(query_int(&scene, "edge_count"), 3);
        });
    }

    #[test]
    fn r838_keyboard_nudges_and_deletes_selected() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = Modifiers::empty();
            // No selection → arrow keys are a no-op.
            assert!(!NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "ArrowRight", m));
            // Select node 0, nudge right, verify it moved by NUDGE_STEP.
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let _ = node
                    .handle
                    .introspect_mut()
                    .unwrap()
                    .intervene("selected", IntrospectValue::Int(0));
            }
            let x0 = query_int(&scene, "node.0.x");
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "ArrowRight", m));
            let x1 = query_int(&scene, "node.0.x");
            assert_eq!(x1 - x0, i64::from(NUDGE_STEP));
            // Delete removes the selected node.
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "Delete", m));
            assert_eq!(graph_intro(&scene).query("node_count"), Some(IntrospectValue::Int(3)));
            assert_eq!(graph_intro(&scene).query("selected"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r838_escape_clears_selection() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let _ = node.handle.introspect_mut().unwrap().intervene("selected", IntrospectValue::Int(2));
            let m = Modifiers::empty();
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "Escape", m));
            assert_eq!(graph_intro(&scene).query("selected"), Some(IntrospectValue::Null));
        });
    }

    /// A fresh coordinator over the shared Owner::cache holders (mutations
    /// persist across instances within one Owner scope).
    fn coordinator() -> NodeGraphExternal {
        NodeGraphExternal::new(
            use_nodes(),
            use_edges(),
            use_selection(),
            use_preview(),
            use_next_edge_id(),
            use_next_node_id(),
        )
    }

    #[test]
    fn r839_point_near_edge_is_curve_distance() {
        let from = (164, 114);
        let to = (256, 154);
        // The endpoints lie exactly on the curve.
        assert!(point_near_edge(from.0, from.1, from, to), "start on curve");
        assert!(point_near_edge(to.0, to.1, from, to), "end on curve");
        // A point far above the wire is not near it.
        assert!(!point_near_edge(210, 30, from, to), "far point misses");
    }

    #[test]
    fn r839_hit_test_edge_finds_the_wire_under_a_click() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let nodes = default_nodes();
            // Midpoint of edge 0 (Texture.out0 -> Multiply.in0) sits in open space.
            let from = output_port_center(&nodes[0], 0);
            let to = input_port_center(&nodes[2], 0);
            let mid = cubic_at(
                (f64::from(from.0), f64::from(from.1)),
                {
                    let (c1, _) = edge_curve(from, to);
                    (f64::from(c1.0), f64::from(c1.1))
                },
                {
                    let (_, c2) = edge_curve(from, to);
                    (f64::from(c2.0), f64::from(c2.1))
                },
                (f64::from(to.0), f64::from(to.1)),
                0.5,
            );
            let coord = coordinator();
            assert_eq!(coord.hit_test_edge(round_i32(mid.0), round_i32(mid.1)), Some(EdgeId(0)));
            assert_eq!(coord.hit_test_edge(10, 10), None, "empty corner hits nothing");
        });
    }

    #[test]
    fn r840_node_and_edge_selection_are_one_sum_type() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            coord.select_node(Some(NodeId(2)));
            assert_eq!(use_selection().get(), Selection::Node(NodeId(2)));
            coord.select_edge(Some(EdgeId(1)));
            assert_eq!(
                use_selection().get(),
                Selection::Edge(EdgeId(1)),
                "selecting an edge replaces the node",
            );
            // The illegal "both selected" state is unrepresentable by construction.
        });
    }

    #[test]
    fn r841_remove_edge_keeps_other_selections_stable() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            // With stable ids, removing a *different* edge does NOT renumber the
            // selected one (the whole point — no index shift).
            coord.select_edge(Some(EdgeId(2)));
            assert!(coord.remove_edge(EdgeId(0)), "remove edge id 0");
            assert_eq!(use_selection().get(), Selection::Edge(EdgeId(2)), "id 2 still selected");
            // Removing the selected edge itself prunes the selection.
            assert!(coord.remove_edge(EdgeId(2)), "remove the selected edge");
            assert_eq!(use_selection().get(), Selection::None);
            assert!(!coord.remove_edge(EdgeId(99)), "unknown edge id rejected");
        });
    }

    #[test]
    fn r839_delete_selected_prefers_the_selected_edge() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            coord.select_edge(Some(EdgeId(0)));
            assert!(coord.delete_selected(), "delete the selected edge");
            assert_eq!(use_edges().get().len(), 2, "one edge removed");
            assert_eq!(use_selection().get(), Selection::None);
        });
    }

    #[test]
    fn r841_delete_node_keeps_survivor_ids_stable() {
        Owner::new().run(|| {
            let coord = coordinator();
            // Select Output (id 3); delete Color (id 1).
            coord.select_node(Some(NodeId(3)));
            assert!(coord.delete_node(NodeId(1)), "delete node id 1");
            // No reindex: Multiply is STILL id 2; its edges keep their ids.
            assert_eq!(coord.node_by_id(NodeId(2)).map(|n| n.title), Some("Multiply".to_owned()));
            assert!(coord.node_by_id(NodeId(1)).is_none(), "Color is gone");
            let edges = use_edges().get();
            assert_eq!(edges.len(), 2, "Color's incident edge dropped");
            assert!(edges.iter().any(|e| e.id == EdgeId(0) && e.from_node == NodeId(0) && e.to_node == NodeId(2)));
            // The selection (Output id 3) is untouched — it did not shift.
            assert_eq!(use_selection().get(), Selection::Node(NodeId(3)));
        });
    }

    #[test]
    fn r842_dynamic_edge_id_seed_is_derived_from_defaults() {
        // The mint seed must be one past the highest default edge id, derived
        // (not a hand-maintained const) so adding a seed edge can never collide.
        let max_default = default_edges().iter().map(|e| e.id.raw()).max().unwrap();
        assert_eq!(first_dynamic_edge_id(), max_default + 1);
        // A freshly minted edge id is distinct from every default edge id.
        Owner::new().run(|| {
            let coord = coordinator();
            assert!(coord.add_edge(NodeId(0), 0, NodeId(3), 0), "add a new edge");
            let default_ids: Vec<u32> = default_edges().iter().map(|e| e.id.raw()).collect();
            let live_ids = live_edge_ids(&coord);
            let minted: Vec<u32> = live_ids.iter().copied().filter(|id| !default_ids.contains(id)).collect();
            assert_eq!(minted.len(), 1, "exactly one minted id");
            assert!(minted[0] > max_default, "minted id is above all default ids");
        });
    }

    #[test]
    fn r849_first_dynamic_node_id_is_derived_from_defaults() {
        // Mirrors the edge-id seed: one past the highest default node id, derived
        // so adding a seed node can never collide a minted id.
        let max_default = default_nodes().iter().map(|n| n.id.raw()).max().unwrap();
        assert_eq!(first_dynamic_node_id(), max_default + 1);
    }

    #[test]
    fn r849_add_node_mints_a_fresh_stable_id_selects_it_and_guards_kind() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            assert_eq!(coord.node_count(), 4);
            // Add a Multiply (palette index 2): id = first dynamic, count 5.
            let id = coord.add_node(2).expect("Multiply is a valid kind");
            assert_eq!(id, NodeId(first_dynamic_node_id()));
            assert_eq!(coord.node_count(), 5);
            assert_eq!(use_selection().get(), Selection::Node(id), "the new node is selected");
            let n = coord.node_by_id(id).expect("new node present");
            assert_eq!(n.title, "Multiply");
            assert_eq!((n.inputs, n.outputs), (2, 1));
            // An out-of-range kind adds nothing.
            assert_eq!(coord.add_node(99), None);
            assert_eq!(coord.node_count(), 5);
        });
    }

    #[test]
    fn r849_added_node_ids_are_monotonic_never_reused_after_delete() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let a = coord.add_node(0).expect("Texture"); // first dynamic id
            assert!(coord.delete_node(a), "remove the just-added node");
            let b = coord.add_node(0).expect("Texture again");
            assert!(b.raw() > a.raw(), "a deleted id is never reused (monotonic mint)");
        });
    }

    #[test]
    fn r849_add_node_rpc_returns_the_new_id_and_rejects_unknown_kinds() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspect");
            // Create by kind NAME; returns the new stable id (AI-first one-shot).
            let id = match intro.invoke("add_node", IntrospectValue::Text("Add".to_owned())) {
                Ok(IntrospectValue::Int(i)) => i,
                other => panic!("expected the new id, got {other:?}"),
            };
            assert_eq!(id, i64::from(first_dynamic_node_id()));
            assert_eq!(intro.query("node_count"), Some(IntrospectValue::Int(5)));
            assert_eq!(
                intro.query(&format!("node.{id}.title")),
                Some(IntrospectValue::Text("Add".to_owned())),
            );
            assert_eq!(intro.query(&format!("node.{id}.inputs")), Some(IntrospectValue::Int(2)));
            // An unknown kind is Rejected; the graph is unchanged.
            assert_eq!(
                intro.invoke("add_node", IntrospectValue::Text("Bogus".to_owned())),
                Err(InvokeError::Rejected),
            );
            assert_eq!(intro.query("node_count"), Some(IntrospectValue::Int(5)));
            // node_ids enumerates the new sparse id (read/write symmetry).
            match intro.query("node_ids") {
                Some(IntrospectValue::Text(s)) => {
                    assert!(s.split(',').any(|t| t == id.to_string()), "node_ids lists the added id: {s}");
                }
                other => panic!("expected node_ids string, got {other:?}"),
            }
        });
    }

    #[test]
    fn r849_palette_card_release_adds_a_node() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let mut coord = coordinator();
            assert_eq!(coord.node_count(), 4);
            // A palette card press+release creates the node (activation on release);
            // the press alone does not.
            coord.handle_send("palette_2:PointerDown");
            assert_eq!(coord.node_count(), 4, "the press alone adds nothing");
            coord.handle_send("palette_2:PointerUp");
            assert_eq!(coord.node_count(), 5, "releasing the palette card created a node");
            let id = use_selection().get().node().expect("new node selected");
            assert_eq!(coord.node_by_id(id).expect("present").title, "Multiply");
        });
    }

    /// Test helper — the live edge id set via the RPC enumeration.
    fn live_edge_ids(coord: &NodeGraphExternal) -> Vec<u32> {
        match coord.query("edge_ids") {
            Some(IntrospectValue::Text(s)) if !s.is_empty() => {
                s.split(',').map(|x| x.parse().unwrap()).collect()
            }
            _ => Vec::new(),
        }
    }

    #[test]
    fn r841_node_ids_and_edge_ids_enumerate_the_sparse_space() {
        Owner::new().run(|| {
            let coord = coordinator();
            assert_eq!(coord.query("node_ids"), Some(IntrospectValue::Text("0,1,2,3".to_owned())));
            assert_eq!(coord.query("edge_ids"), Some(IntrospectValue::Text("0,1,2".to_owned())));
            // Delete node id 1 → the id space stays sparse, no renumber.
            coord.delete_node(NodeId(1));
            assert_eq!(
                coord.query("node_ids"),
                Some(IntrospectValue::Text("0,2,3".to_owned())),
                "sparse, no renumber",
            );
        });
    }

    #[test]
    fn r839_background_press_probe_selects_a_wire() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // A bare background press, a capture-seed move onto edge 0's
            // midpoint, then a bare release selects that wire.
            send(&mut scene, "PointerDown");
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                node.handle.pointer_move(0.328_125, 0.319); // ~ (210, 134) = edge 0 midpoint
            }
            send(&mut scene, "PointerUp");
            assert_eq!(query_int(&scene, "selected_edge"), 0, "wire selected");
            assert_eq!(graph_intro(&scene).query("selected"), Some(IntrospectValue::Null));
            // A bare press on empty space deselects.
            send(&mut scene, "PointerDown");
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                node.handle.pointer_move(0.95, 0.95); // empty corner
            }
            send(&mut scene, "PointerUp");
            assert_eq!(graph_intro(&scene).query("selected_edge"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r838_view_carries_graph_and_node_and_edge_tags() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let scene = view((), &Frame::new());
            assert!(scene.contains_tag(GRAPH_TAG), "graph root painted");
            assert!(scene.contains_tag(&format!("{GRAPH_TAG}#node_0")), "node 0 painted");
            assert!(scene.contains_tag(&format!("{GRAPH_TAG}#oport_0_0")), "node 0 output port painted");
            assert!(scene.contains_tag(&format!("{GRAPH_TAG}#iport_2_0")), "node 2 input port painted");
            assert!(scene.contains_tag(&format!("{GRAPH_TAG}#edge_0")), "edge 0 painted");
        });
    }

    #[test]
    fn r840_access_node_emits_group_not_ordered_list() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_selection().set(Selection::Node(NodeId(2)));
            let nodes = NodeEditorView::access_node(&(), Some(GRAPH_TAG));
            // R849 — root + palette toolbar + 5 palette buttons + graph group +
            // one generic per node.
            assert_eq!(nodes.len(), 1 + 1 + PALETTE.len() + 1 + 4, "root + palette + graph");
            // The root wraps the palette + the canvas; the focusable canvas is
            // the graph group (found by tag, not position).
            assert_eq!(nodes[0].role, AriaRole::Group, "editor root is a group");
            assert_eq!(nodes[0].tag, ROOT_TAG);
            let palette = nodes.iter().find(|n| n.tag == PALETTE_TAG).expect("palette toolbar present");
            assert_eq!(palette.role, AriaRole::Toolbar);
            let add_texture = nodes
                .iter()
                .find(|n| n.tag == format!("{GRAPH_TAG}#palette_0"))
                .expect("Texture palette button present");
            assert_eq!(add_texture.role, AriaRole::Button);
            assert_eq!(add_texture.name.as_deref(), Some("Add Texture"));
            // R850 — via the toolbar_button_nodes SSOT, palette buttons carry
            // roving-set metadata the hand-rolled version lacked.
            assert_eq!(add_texture.position_in_set, Some(1), "1-based posinset");
            assert_eq!(
                add_texture.size_of_set,
                Some(u32::try_from(PALETTE.len()).unwrap()),
                "setsize = palette length",
            );
            let graph = nodes.iter().find(|n| n.tag == GRAPH_TAG).expect("graph group present");
            // R840 audit fix: a graph is an unordered set, so Group/Generic —
            // never List/ListItem with a false aria-posinset.
            assert_eq!(graph.role, AriaRole::Group);
            assert!(graph.state.focused, "the canvas is the focused tab stop");
            let multiply = nodes
                .iter()
                .find(|n| n.tag == format!("{GRAPH_TAG}#node_2"))
                .expect("Multiply node present");
            assert_eq!(multiply.role, AriaRole::Generic);
            assert_eq!(multiply.name.as_deref(), Some("Multiply (2 in, 1 out)"));
            assert_eq!(multiply.selected, Some(true));
            assert_eq!(multiply.position_in_set, None, "no false ordered-set position");
        });
    }

    #[test]
    fn r838_view_contains_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<NodeEditorView>(
            (),
            &Frame::default(),
        );
    }
}
