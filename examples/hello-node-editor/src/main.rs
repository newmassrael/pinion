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
//! - **Undo / redo** (R851 + R853): every edit is reversible on the shared
//!   [`UndoStack`] — the **structural** edits (add node, delete node + its
//!   incident edges, connect, disconnect) as [`GraphEdit`] deltas, and node
//!   **moves** (drag / nudge / `intervene .x`) as [`MoveNodeCmd`]s. A drag is
//!   recorded as one move at gesture end; a keyboard nudge **burst** coalesces
//!   to one undo step (the `UndoCommand::merge` hook). `Ctrl+Z` /
//!   `Ctrl+Shift+Z` (`Ctrl+Y`) and the AI-first [`UndoStackExternal`] drive it.
//! - **Persistence** (R852): the graph saves / loads through the §3 §5.15
//!   [`Storage`] substrate — `save` / `load` write and restore a
//!   [`SerializedGraph`] JSON blob (nodes + edges + the monotonic id counters)
//!   via [`build_graph_storage`] (a `FileStorage` when the platform offers a
//!   data dir, the in-memory fallback otherwise — the todomvc runtime-selection
//!   idiom, so the binding really reaches the file backend). The same snapshot
//!   is the AI-first `query serialized` / `invoke set_graph` read-write pair,
//!   and `Ctrl+S` / `Ctrl+O` drive save / open. Loading a graph clears the undo
//!   history (the opened document is a fresh baseline — the `QUndoStack` model).
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
use pinion_core::storage::Storage;
use pinion_core::undo::{use_undo_stack, UndoCommand, UndoStack, UndoStackExternal};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, PathStyle, Size,
    Stroke, StrokeCap, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_platform_storage::{open_app_storage, FileStorage};
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

/// R851 — the [`UndoStackExternal`] anchor: the AI-first undo-history surface
/// (`query can_undo` / `index` / `undo_label`; `invoke undo` / `redo` / `clear`),
/// reached at `/node_undo/external/<slot>`. No `#` in the tag, so it routes as
/// its own coordinator (never confused with a `node_graph#…` composite).
const UNDO_TAG: &str = "node_undo";
/// R851 — the [`use_undo_stack`] cache key: the reducer-side recorder (the
/// coordinator), the keyboard `Ctrl+Z` path, and the [`UndoStackExternal`] all
/// resolve the same shared [`UndoStack`] from this key.
const UNDO_KEY: &str = "node_graph.undo";

/// R852 — the per-OS data dir name for the file-backed graph store
/// ([`open_app_storage`]); the `Owner::cache` key for the shared storage hook.
const STORAGE_APP_NAME: &str = "pinion-node-editor";
const STORAGE_CACHE_KEY: &str = "node_graph.storage";
/// R852 — the single [`Storage`] key the whole graph snapshot is written under
/// (one blob, so `FileStorage`'s tempfile + rename covers the whole save).
const STORAGE_KEY: &str = "node_graph.state";
/// R852 — env override pointing at a custom storage root (the demo's tempdir,
/// shared by the todomvc persistence demos so launches do not contaminate the
/// developer's real data dir).
const STORAGE_DIR_ENV: &str = "PINION_STORAGE_DIR";
/// R852 — bump on an incompatible [`SerializedGraph`] layout change; a load of a
/// mismatched version starts fresh (silent fall-through, the todomvc precedent).
const PERSISTED_SCHEMA_VERSION: u32 = 1;

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

/// R852 — the persistable graph snapshot. Carries the nodes, the edges, **and**
/// the monotonic id counters, so a reload resumes minting where the saved
/// session left off (a deleted-then-saved id is never handed out again). The
/// selection is transient UI state and is *not* persisted. `schema_version`
/// gates the load: a mismatch starts fresh rather than misreading an old layout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SerializedGraph {
    schema_version: u32,
    nodes: Vec<GraphNode>,
    edges: Vec<Edge>,
    next_node_id: u32,
    next_edge_id: u32,
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

/// R851 — the shared [`UndoStack`] for this editor scope. Reached identically
/// by the coordinator (which records [`GraphEdit`]s in its mutation methods),
/// the keyboard `Ctrl+Z` path, the status-line `undo_label` read, and the
/// [`UndoStackExternal`] — one history source of truth ([`use_undo_stack`]).
fn use_undo() -> Rc<UndoStack> {
    use_undo_stack(UNDO_KEY)
}

/// R852 — sized newtype around `Box<dyn Storage>` so the `Owner::cache` slot
/// holds one concrete type while the runtime impl (`FileStorage` /
/// [`InMemoryStorage`](pinion_core::storage::InMemoryStorage)) hides in the box
/// (the todomvc `AppStorage` shape, [[owner-cache-typed-key]]).
struct GraphStorage(Box<dyn Storage>);

impl Storage for GraphStorage {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.0.load(key)
    }
    fn save(&self, key: &str, bytes: &[u8]) {
        self.0.save(key, bytes);
    }
    fn remove(&self, key: &str) {
        self.0.remove(key);
    }
}

/// R852 — build the platform storage once. Priority: the [`STORAGE_DIR_ENV`]
/// override (the demo's tempdir) → the per-OS data dir ([`open_app_storage`]) →
/// the in-memory fallback (headless / sandboxed). The runtime selection is what
/// makes `save` / `load` actually reach the file backend rather than a hardcoded
/// in-memory stub (the R834 "binding must reach the real backend" lesson).
fn build_graph_storage() -> Box<dyn Storage> {
    if let Ok(custom_dir) = std::env::var(STORAGE_DIR_ENV) {
        match FileStorage::try_new(std::path::PathBuf::from(&custom_dir)) {
            Ok(file) => return Box::new(file) as Box<dyn Storage>,
            Err(e) => eprintln!(
                "hello-node-editor: FileStorage at {custom_dir:?} via {STORAGE_DIR_ENV} failed ({e}); \
                 falling back to the default data dir",
            ),
        }
    }
    match open_app_storage(STORAGE_APP_NAME) {
        Ok(file) => Box::new(file) as Box<dyn Storage>,
        Err(e) => {
            eprintln!(
                "hello-node-editor: open_app_storage({STORAGE_APP_NAME:?}) failed ({e}); \
                 persistence will be in-memory only (lost on exit)",
            );
            Box::new(pinion_core::storage::InMemoryStorage::new()) as Box<dyn Storage>
        }
    }
}

/// R852 — the shared graph store. The coordinator (which `save`s / `load`s) is
/// the only consumer today; the hook keeps the platform handle a process
/// singleton so a future status / autosave consumer reaches the same instance.
///
/// `GraphStorage` + [`build_graph_storage`] mirror todomvc's `AppStorage` +
/// `build_app_storage` — this is the **2nd** inline copy of the boxed-dyn +
/// env-override + in-memory-fallback pattern. The convention (clipboard's
/// `pinion_platform_clipboard::use_app_clipboard`, lifted at R790 once the
/// copies multiplied) is to lift a `use_app_storage(key, app)` into
/// `pinion-platform-storage` at the **3rd** consumer; the fallback chain is
/// mechanical wiring (a policy, not a correctness invariant), so 2 copies stay
/// inline per the Rule of Three (documented carry, not silent debt).
fn use_graph_storage() -> Rc<GraphStorage> {
    Owner::current()
        .expect("use_graph_storage requires an active Owner scope")
        .cache(STORAGE_CACHE_KEY, || GraphStorage(build_graph_storage()))
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

// ─── reversible structural edit (the UndoCommand) ──────────────────

/// R851 §5.52 — one reversible **structural** graph edit, recorded onto the
/// shared [`UndoStack`]. A *granular delta* (only the entities that changed),
/// never a whole-graph snapshot ([[granular-undo-not-snapshot]]): an add carries
/// its new entities in `added_*`, a delete carries the removed entities (a node
/// *and* its incident edges) in `removed_*`, and a connect that displaces a wire
/// (the single-wire input rule) carries both. `redo` removes the `removed_*` and
/// adds the `added_*`; `undo` is the exact inverse with the sets swapped, so the
/// stored entities (with their stable ids) round-trip byte-for-byte — a redone
/// node keeps its original [`NodeId`], a restored edge its [`EdgeId`].
/// The entity delta of one [`GraphEdit`]: what the edit adds and what it
/// removes. `add_*` are present *after* the edit but not before; `remove_*` are
/// present *before* but not after, stored verbatim so `undo` re-inserts them
/// with their original stable ids. A connect that displaces a wire fills both
/// (`added_edges` = the new wire, `removed_edges` = the displaced one).
#[derive(Default)]
struct GraphDelta {
    added_nodes: Vec<GraphNode>,
    added_edges: Vec<Edge>,
    removed_nodes: Vec<GraphNode>,
    removed_edges: Vec<Edge>,
}

struct GraphEdit {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    edges: Rc<Signal<Vec<Edge>>>,
    selection: Rc<Signal<Selection>>,
    label: Cow<'static, str>,
    delta: GraphDelta,
    sel_before: Selection,
    sel_after: Selection,
}

impl GraphEdit {
    /// Drop `rm_*` by stable id, append `add_*`, then set the selection — the
    /// shared body of [`redo`](UndoCommand::redo) / [`undo`](UndoCommand::undo)
    /// with the add / remove roles swapped. The signal writes are gated on a
    /// non-empty delta so an edge-only edit never reclones the node vector
    /// (and vice-versa), keeping the repaint minimal.
    fn apply(
        &self,
        rm_nodes: &[GraphNode],
        add_nodes: &[GraphNode],
        rm_edges: &[Edge],
        add_edges: &[Edge],
        sel: Selection,
    ) {
        if !rm_nodes.is_empty() || !add_nodes.is_empty() {
            self.nodes.set_with(|prev| {
                let mut next: Vec<GraphNode> =
                    prev.iter().filter(|n| !rm_nodes.iter().any(|r| r.id == n.id)).cloned().collect();
                next.extend(add_nodes.iter().cloned());
                next
            });
        }
        if !rm_edges.is_empty() || !add_edges.is_empty() {
            self.edges.set_with(|prev| {
                let mut next: Vec<Edge> =
                    prev.iter().copied().filter(|e| !rm_edges.iter().any(|r| r.id == e.id)).collect();
                next.extend(add_edges.iter().copied());
                next
            });
        }
        self.selection.set(sel);
    }
}

impl UndoCommand for GraphEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        let d = &self.delta;
        self.apply(&d.removed_nodes, &d.added_nodes, &d.removed_edges, &d.added_edges, self.sel_after);
    }

    fn undo(&self) {
        let d = &self.delta;
        self.apply(&d.added_nodes, &d.removed_nodes, &d.added_edges, &d.removed_edges, self.sel_before);
    }
}

/// R853 §5.52 — a reversible node **move** (the position-edit `UndoCommand`,
/// distinct from the structural [`GraphEdit`]). A move stores only the moved
/// node's `before` / `after` window position, so undo / redo is an O(1) reposition
/// — never a graph-wide delta. It opts into the [`UndoCommand`] coalescing hook
/// (`merge`): a `coalescable` move folds a contiguous same-node move into itself,
/// so a keyboard nudge **burst** collapses to one undo step (the canonical
/// editor behaviour). A drag is recorded as one *non*-coalescable move at gesture
/// end (it is already the whole gesture), so it neither absorbs nor is absorbed
/// by an adjacent nudge run.
struct MoveNodeCmd {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    id: NodeId,
    before: (i32, i32),
    after: (i32, i32),
    coalescable: bool,
}

impl MoveNodeCmd {
    /// Set the moved node's position (no clamp — `before` / `after` were
    /// captured from already-clamped positions). A no-op if the node is absent
    /// (a LIFO undo can never reach a move while the node is deleted, but the
    /// signal write stays total either way).
    fn set_pos(&self, pos: (i32, i32)) {
        self.nodes.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(n) = next.iter_mut().find(|n| n.id == self.id) {
                n.x = pos.0;
                n.y = pos.1;
            }
            next
        });
    }
}

impl UndoCommand for MoveNodeCmd {
    fn label(&self) -> Cow<'static, str> {
        Cow::Borrowed("Move node")
    }

    fn redo(&self) {
        self.set_pos(self.after);
    }

    fn undo(&self) {
        self.set_pos(self.before);
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }

    /// Fold a contiguous same-node move into this one: extend `after` to the
    /// successor's. Only when both ends are `coalescable` and target the same
    /// node — so a drag (non-coalescable) breaks the run on either side, and a
    /// move of a different node starts a fresh undo step.
    fn merge(&mut self, next: &dyn UndoCommand) -> bool {
        let Some(next) = next.as_any().and_then(|a| a.downcast_ref::<MoveNodeCmd>()) else {
            return false;
        };
        // R856 — require contiguity (`self.after == next.before`) like the
        // substrate's canonical `AddCmd::merge`: every current caller captures
        // `before` from live state, so the run is contiguous by construction, but
        // the guard keeps a future stale-`before` caller from folding a
        // non-contiguous run and losing the intermediate position on undo.
        if self.coalescable && next.coalescable && self.id == next.id && self.after == next.before {
            self.after = next.after;
            true
        } else {
            false
        }
    }
}

/// R851 — prune a selection that a structural edit just made dangling: a
/// [`Selection`] over a removed node / edge id collapses to [`Selection::None`]
/// (the stable-id analogue of R838's index-shift bookkeeping — "is it still
/// alive?" is the whole adjustment). Computed at record time as the edit's
/// `sel_after`, so the [`GraphEdit`] carries the post-edit selection explicitly
/// rather than re-deriving it on each replay.
fn validate_after(sel: Selection, removed_nodes: &[NodeId], removed_edges: &[EdgeId]) -> Selection {
    match sel {
        Selection::Node(id) if removed_nodes.contains(&id) => Selection::None,
        Selection::Edge(id) if removed_edges.contains(&id) => Selection::None,
        other => other,
    }
}

// ─── coordinator External ──────────────────────────────────────────

/// R852 — the cross-cutting services the coordinator depends on, distinct from
/// the reactive model holders: the shared undo history and the persistence
/// backend. Bundled so [`NodeGraphExternal::new`] stays within the argument
/// budget while the model holders remain explicit.
struct GraphServices {
    undo: Rc<UndoStack>,
    storage: Rc<GraphStorage>,
}

/// The node-graph coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) plus the internal drag latches.
struct NodeGraphExternal {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    edges: Rc<Signal<Vec<Edge>>>,
    /// The single selection (node | edge | none) — a sum type over stable ids,
    /// so node/edge selection is mutually exclusive AND survives an unrelated
    /// delete (a dangling selection is pruned to `None` by [`validate_after`]
    /// at record time, carried as the edit's `sel_after`).
    selection: Rc<Signal<Selection>>,
    preview: Rc<Signal<Option<Preview>>>,
    /// Monotonic [`EdgeId`] source for newly-connected wires.
    next_edge_id: Rc<Cell<u32>>,
    /// R849 — monotonic [`NodeId`] source for newly created (palette / RPC) nodes.
    next_node_id: Rc<Cell<u32>>,
    /// R851 — the shared undo history every structural mutation records onto
    /// (the same `Rc` the [`UndoStackExternal`] and the keyboard path reach).
    undo: Rc<UndoStack>,
    /// R852 — the platform graph store behind `save` / `load`.
    storage: Rc<GraphStorage>,
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
        services: GraphServices,
    ) -> Self {
        Self {
            nodes,
            edges,
            selection,
            preview,
            next_edge_id,
            next_node_id,
            undo: services.undo,
            storage: services.storage,
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

    /// R851 — build a [`GraphEdit`] over the shared signals and `record` it onto
    /// the undo stack. `record` applies the edit forward (its `redo`), so this
    /// is the single mutation path for every *structural* change — the caller
    /// supplies the delta + the before / after selection and never touches the
    /// signals directly ([`UndoStack::record`] semantics).
    fn record_edit(
        &self,
        label: impl Into<Cow<'static, str>>,
        delta: GraphDelta,
        sel_before: Selection,
        sel_after: Selection,
    ) {
        self.undo.record(GraphEdit {
            nodes: Rc::clone(&self.nodes),
            edges: Rc::clone(&self.edges),
            selection: Rc::clone(&self.selection),
            label: label.into(),
            delta,
            sel_before,
            sel_after,
        });
    }

    /// R853 — journal a node move onto the undo stack. The position is already
    /// applied (the live drag / nudge / intervene set it), so this `push_applied`s
    /// the [`MoveNodeCmd`] without re-applying; the stack's coalescing folds a
    /// contiguous `coalescable` run (a nudge burst) into one step.
    fn record_move(&self, id: NodeId, before: (i32, i32), after: (i32, i32), coalescable: bool) {
        if before == after {
            return;
        }
        self.undo.push_applied(MoveNodeCmd { nodes: Rc::clone(&self.nodes), id, before, after, coalescable });
    }

    /// R852 — snapshot the persistable graph (nodes + edges + the monotonic id
    /// counters; the selection is transient and omitted).
    fn snapshot(&self) -> SerializedGraph {
        SerializedGraph {
            schema_version: PERSISTED_SCHEMA_VERSION,
            nodes: self.nodes.get(),
            edges: self.edges.get(),
            next_node_id: self.next_node_id.get(),
            next_edge_id: self.next_edge_id.get(),
        }
    }

    /// R852 — the graph as a JSON string (the AI-first `query serialized` read).
    fn serialized_json(&self) -> String {
        serde_json::to_string(&self.snapshot()).unwrap_or_default()
    }

    /// R852 — replace the whole graph from a snapshot: swap nodes / edges, resume
    /// the id counters where the snapshot left off, drop the selection / preview,
    /// and clear the undo history — the opened document is a fresh baseline (the
    /// `QUndoStack` "open clears the stack" model). The single restore path
    /// behind `set_graph` / `load`, so every entry point clears undo identically.
    fn apply_snapshot(&self, g: SerializedGraph) {
        self.nodes.set(g.nodes);
        self.edges.set(g.edges);
        self.next_node_id.set(g.next_node_id);
        self.next_edge_id.set(g.next_edge_id);
        self.selection.set(Selection::None);
        self.preview.set(None);
        self.undo.clear();
        // R856 — reset the gesture latches WITHOUT recording: a document
        // replacement is not a drag commit, so `end_gesture` (which would journal
        // a stale MoveNodeCmd onto the just-cleared stack) must not run here.
        self.reset_gesture();
    }

    /// R852 — parse + apply a JSON snapshot (the AI-first `set_graph` write, the
    /// inverse of [`serialized_json`](Self::serialized_json)). Rejects malformed
    /// JSON or a schema-version mismatch (`false`, the graph unchanged).
    fn load_json(&self, json: &str) -> bool {
        let Ok(g) = serde_json::from_str::<SerializedGraph>(json) else {
            return false;
        };
        if g.schema_version != PERSISTED_SCHEMA_VERSION {
            return false;
        }
        self.apply_snapshot(g);
        true
    }

    /// R852 — persist the current graph to [`STORAGE_KEY`] (the `Ctrl+S` / RPC
    /// `save` path). The single blob keeps `FileStorage`'s tempfile + rename
    /// covering the whole transaction. Returns whether the snapshot serialized.
    fn save(&self) -> bool {
        let Ok(bytes) = serde_json::to_vec(&self.snapshot()) else {
            return false;
        };
        self.storage.save(STORAGE_KEY, &bytes);
        true
    }

    /// R852 — restore the graph from [`STORAGE_KEY`] (the `Ctrl+O` / RPC `load`
    /// path). `false` (graph unchanged) when nothing is stored or the blob is
    /// unreadable / a version mismatch.
    fn load(&self) -> bool {
        let Some(bytes) = self.storage.load(STORAGE_KEY) else {
            return false;
        };
        match std::str::from_utf8(&bytes) {
            Ok(json) => self.load_json(json),
            Err(_) => false,
        }
    }

    /// Move node `id` to a clamped `(x, y)` — the low-level reposition behind the
    /// capture drag, the arrow nudge, and the `intervene node.<id>.{x,y}` path.
    /// It does *not* journal (a drag calls it many times per gesture); R853 — the
    /// callers (`end_gesture` / `nudge_selected` / the `x`,`y` intervene arm)
    /// record the [`MoveNodeCmd`] once the gesture / keystroke settles.
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
        let node = GraphNode { id, title: title.to_owned(), x, y, inputs, outputs };
        let sel_before = self.selection.get();
        // `record` applies the edit forward — pushing the node and selecting it
        // (the prior direct writes) — so a single Ctrl+Z removes it again.
        self.record_edit(
            format!("Add {title}"),
            GraphDelta { added_nodes: vec![node], ..GraphDelta::default() },
            sel_before,
            Selection::Node(id),
        );
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
        let edges = self.edges.get();
        let dup = edges.iter().any(|e| {
            e.from_node == from_node
                && e.from_port == from_port
                && e.to_node == to_node
                && e.to_port == to_port
        });
        if dup {
            return false;
        }
        // Input single-wire rule: the new wire displaces any existing wire into
        // the same target input — captured as a removed delta so undo restores it.
        let replaced: Vec<Edge> =
            edges.iter().copied().filter(|e| e.to_node == to_node && e.to_port == to_port).collect();
        let id = EdgeId(self.next_edge_id.get());
        self.next_edge_id.set(id.raw() + 1);
        let new_edge = Edge { id, from_node, from_port, to_node, to_port };
        let sel_before = self.selection.get();
        // Displacing a wire may strand a selected edge — prune it post-edit.
        let removed_ids: Vec<EdgeId> = replaced.iter().map(|e| e.id).collect();
        let sel_after = validate_after(sel_before, &[], &removed_ids);
        self.record_edit(
            "Connect",
            GraphDelta { added_edges: vec![new_edge], removed_edges: replaced, ..GraphDelta::default() },
            sel_before,
            sel_after,
        );
        true
    }

    /// Remove the edge with stable id `id` (no-op + `false` if absent). R851 —
    /// the edge is stored as a removed delta so undo re-inserts it verbatim.
    fn remove_edge(&self, id: EdgeId) -> bool {
        let Some(edge) = self.edges.get().iter().copied().find(|e| e.id == id) else {
            return false;
        };
        let sel_before = self.selection.get();
        let sel_after = validate_after(sel_before, &[], &[id]);
        self.record_edit(
            "Disconnect",
            GraphDelta { removed_edges: vec![edge], ..GraphDelta::default() },
            sel_before,
            sel_after,
        );
        true
    }

    /// Delete node `id` and its incident edges. No reindex: every surviving
    /// node and edge keeps its stable id, so references elsewhere stay valid.
    /// R851 — the node *and* its incident edges are captured as a removed delta,
    /// so one Ctrl+Z restores the node together with every wire it carried.
    fn delete_node(&self, id: NodeId) -> bool {
        let Some(node) = self.nodes.get().iter().find(|n| n.id == id).cloned() else {
            return false;
        };
        let incident: Vec<Edge> =
            self.edges.get().iter().copied().filter(|e| e.from_node == id || e.to_node == id).collect();
        let sel_before = self.selection.get();
        let incident_ids: Vec<EdgeId> = incident.iter().map(|e| e.id).collect();
        let sel_after = validate_after(sel_before, &[id], &incident_ids);
        self.record_edit(
            "Delete node",
            GraphDelta { removed_nodes: vec![node], removed_edges: incident, ..GraphDelta::default() },
            sel_before,
            sel_after,
        );
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

    /// Nudge the selected node by `(dx, dy)` (the arrow-key path). R853 — each
    /// nudge journals a *coalescable* move, so a burst of arrow keys collapses to
    /// one undo step.
    fn nudge_selected(&self, dx: i32, dy: i32) -> bool {
        let Some(id) = self.selection.get().node() else {
            return false;
        };
        let Some(node) = self.node_by_id(id) else {
            return false;
        };
        let before = (node.x, node.y);
        if !self.set_node_pos(id, node.x + dx, node.y + dy) {
            return false;
        }
        let after = self.node_by_id(id).map_or(before, |n| (n.x, n.y));
        self.record_move(id, before, after, true);
        true
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
        // R853 — a completed node-body drag (a node was grabbed and a drag
        // snapshot taken) journals as ONE non-coalescable move: before = the grab
        // position, after = the release position. The intermediate `pointer_move`
        // writes are live preview only. Edge-connect drags arm `pending_press`,
        // not `grabbed_node`, so they never record a move here.
        if let (Some(id), Some(start)) = (self.grabbed_node.get(), self.node_drag.get()) {
            if let Some(node) = self.node_by_id(id) {
                self.record_move(id, (start.x_at_press, start.y_at_press), (node.x, node.y), false);
            }
        }
        self.reset_gesture();
    }

    /// R856 — drop the in-flight gesture latches **without** journaling a move.
    /// `end_gesture` records a completed drag before calling this; a whole-graph
    /// replacement ([`apply_snapshot`](Self::apply_snapshot)) calls it directly,
    /// so a `load` / `set_graph` issued mid-drag cannot record a stale move onto
    /// the freshly-cleared undo history (the "load clears undo" contract).
    fn reset_gesture(&self) {
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
            ("serialized", "string"),
            ("set_graph", "string"),
            ("save", "json"),
            ("load", "json"),
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
            // R852 — the whole graph as one JSON blob (the AI-first read; its
            // write-twin is `invoke set_graph`).
            "serialized" => Some(IntrospectValue::Text(self.serialized_json())),
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
                let before = (node.x, node.y);
                if field == "x" {
                    self.set_node_pos(id, coord, node.y);
                } else {
                    self.set_node_pos(id, node.x, coord);
                }
                // R853 — journal the RPC move (coalescable, so an `x` then `y` on
                // the same node fold into one undo step).
                let after = self.node_by_id(id).map_or(before, |n| (n.x, n.y));
                self.record_move(id, before, after, true);
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
            // R852 — replace the graph from a JSON snapshot (the write-twin of
            // `query serialized`); malformed JSON or a version mismatch is
            // Rejected and leaves the graph unchanged.
            "set_graph" => match args {
                IntrospectValue::Text(s) => self
                    .load_json(&s)
                    .then_some(IntrospectValue::Bool(true))
                    .ok_or(InvokeError::Rejected),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R852 — persist to / restore from the Storage backend. `load`
            // returns false (graph unchanged) when nothing is stored yet.
            "save" => Ok(IntrospectValue::Bool(self.save())),
            "load" => Ok(IntrospectValue::Bool(self.load())),
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

/// R851 — map a held-`Ctrl` keystroke to an undo-stack verb: `Ctrl+Z` undoes,
/// `Ctrl+Shift+Z` / `Ctrl+Y` redo (the canonical editor pairing). `None` for any
/// other combination, so the plain-key handling below still runs.
fn undo_redo_verb(key: &str, modifiers: Modifiers) -> Option<&'static str> {
    if !modifiers.control_key() {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "z" if modifiers.shift_key() => Some("redo"),
        "z" => Some("undo"),
        "y" => Some("redo"),
        _ => None,
    }
}

/// R852 — map a held-`Ctrl` keystroke to a persistence verb on the graph
/// coordinator: `Ctrl+S` saves, `Ctrl+O` opens (loads). `None` otherwise.
fn save_load_verb(key: &str, modifiers: Modifiers) -> Option<&'static str> {
    if !modifiers.control_key() {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "s" => Some("save"),
        "o" => Some("load"),
        _ => None,
    }
}

/// R851 — drive `verb` (`undo` / `redo`) on the [`UndoStackExternal`] at
/// [`UNDO_TAG`] — the same SSOT the RPC path drives, so the keyboard adds no
/// hand-rolled undo logic to the graph coordinator. Returns `true` (the editor
/// consumes Ctrl+Z even at a history boundary, where the verb is a harmless
/// no-op) once the undo external is found.
fn invoke_undo(scene: &mut Scene, verb: &str) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(UNDO_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let _ = intro.invoke(verb, IntrospectValue::Null);
    true
}

fn apply_key_graph(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    if let Some(verb) = undo_redo_verb(key, modifiers) {
        return invoke_undo(scene, verb);
    }
    let Some(node) = scene.find_external_with_tag_mut(GRAPH_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    // R852 — Ctrl+S save / Ctrl+O open, on the graph coordinator itself.
    if let Some(verb) = save_load_verb(key, modifiers) {
        let _ = intro.invoke(verb, IntrospectValue::Null);
        return true;
    }
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
    // R851 — surface the next-undo action so the history is a visible witness
    // (and reading it subscribes the view to the undo stack's revision signal,
    // so an undo/redo that only moves the cursor still repaints the status).
    let undo_label = use_undo().undo_label();
    let status = format!(
        "{} nodes · {} edges · selected: {sel_label} · undo: {}",
        nodes.len(),
        edges.len(),
        undo_label.as_deref().unwrap_or("—"),
    );
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
            GraphServices { undo: use_undo(), storage: use_graph_storage() },
        ))
    }

    /// R851 — the AI-first undo-history surface. The [`UndoStackExternal`] wraps
    /// the **same** shared [`UndoStack`] the coordinator records onto (via
    /// [`use_undo`]), so `query`/`invoke` at `/node_undo/external/…` observe and
    /// drive the identical history the canvas + keyboard use (one SSOT). It is a
    /// coordinator-only extra: it paints nothing and is not a focus stop.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(UNDO_TAG, Box::new(UndoStackExternal::new(use_undo())))]
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
        modifiers: Modifiers,
    ) -> bool {
        match focused {
            Some(GRAPH_TAG) => apply_key_graph(scene, key, modifiers),
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
        // Build the primary from `coordinator()` (in-memory storage) rather than
        // `create_external` so a unit test never spins up the real `FileStorage`
        // (which eagerly create_dir_all's the OS data dir).
        Scene::External(ExternalNode::new(Box::new(coordinator()) as Box<dyn External>).with_tag(GRAPH_TAG))
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
    /// R852 — an in-memory `GraphStorage` cached under [`STORAGE_CACHE_KEY`], so
    /// tests exercise `save` / `load` without touching the filesystem (the real
    /// `FileStorage` path is covered by `tools/demos/r852_node_persist.py` via
    /// `isolated_storage_dir`). Shared within the Owner scope, mirroring how
    /// `use_graph_storage` makes the platform handle a process singleton.
    fn mem_storage() -> Rc<GraphStorage> {
        Owner::current()
            .expect("mem_storage requires an active Owner scope")
            .cache(STORAGE_CACHE_KEY, || {
                GraphStorage(Box::new(pinion_core::storage::InMemoryStorage::new()))
            })
    }

    fn coordinator() -> NodeGraphExternal {
        NodeGraphExternal::new(
            use_nodes(),
            use_edges(),
            use_selection(),
            use_preview(),
            use_next_edge_id(),
            use_next_node_id(),
            GraphServices { undo: use_undo(), storage: mem_storage() },
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

    // ── R851 undo / redo (structural edits) ────────────────────────

    /// Modifier state for a held-`Ctrl` (optionally `Shift`) keystroke.
    fn mods(ctrl: bool, shift: bool) -> Modifiers {
        Modifiers { shift, ctrl, alt: false, meta: false }
    }

    /// A scene with the primary coordinator **and** the [`UndoStackExternal`]
    /// extra, both sharing the one `use_undo()` stack — exactly what
    /// `create_external` + `create_extra_externals` wire, so the keyboard /
    /// RPC undo path (which finds [`UNDO_TAG`]) can be exercised in a unit test.
    fn boot_full_scene() -> Scene {
        let primary =
            Scene::External(ExternalNode::new(Box::new(coordinator()) as Box<dyn External>).with_tag(GRAPH_TAG));
        let undo = Scene::External(
            ExternalNode::new(Box::new(UndoStackExternal::new(use_undo()))).with_tag(UNDO_TAG),
        );
        Scene::Container(ContainerNode::new(vec![primary, undo]))
    }

    fn undo_ext_query(scene: &Scene, slot: &str) -> Option<IntrospectValue> {
        scene
            .find_external_with_tag(UNDO_TAG)
            .and_then(|n| n.handle.introspect())
            .expect("undo external present")
            .query(slot)
    }

    #[test]
    fn r851_create_extra_externals_wires_one_undo_surface() {
        Owner::new().run(|| {
            let extras = NodeEditorView::create_extra_externals();
            assert_eq!(extras.len(), 1, "exactly one extra external");
            assert_eq!(extras[0].tag, UNDO_TAG, "the extra is the undo-history surface");
        });
    }

    #[test]
    fn r851_add_node_undo_removes_it_redo_restores_same_id() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            assert!(!stack.can_undo(), "boot: clean history");
            let id = coord.add_node(2).expect("Multiply"); // first dynamic id
            assert_eq!(coord.node_count(), 5);
            assert_eq!(use_selection().get(), Selection::Node(id));
            assert!(stack.can_undo(), "the add is journaled");
            assert_eq!(stack.undo_label().as_deref(), Some("Add Multiply"));

            assert!(stack.undo(), "undo the add");
            assert_eq!(coord.node_count(), 4, "the node is gone");
            assert_eq!(use_selection().get(), Selection::None, "selection reverts");
            assert!(coord.node_by_id(id).is_none(), "the added id is gone");

            assert!(stack.redo(), "redo the add");
            assert_eq!(coord.node_count(), 5, "the node is back");
            assert_eq!(use_selection().get(), Selection::Node(id), "and re-selected");
            assert_eq!(coord.node_by_id(id).expect("present").id, id, "with the SAME stable id");
        });
    }

    #[test]
    fn r851_delete_node_undo_restores_node_and_all_incident_edges() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            // Node 2 (Multiply) is incident to all three seed edges (0, 1, 2).
            assert!(coord.delete_node(NodeId(2)), "delete the central node");
            assert_eq!(coord.node_count(), 3, "node removed");
            assert_eq!(coord.edges.get().len(), 0, "all three incident edges removed");

            assert!(stack.undo(), "undo the delete");
            assert_eq!(coord.node_count(), 4, "the node is restored");
            assert_eq!(coord.edges.get().len(), 3, "every incident edge is restored");
            assert_eq!(
                coord.query("edge.0"),
                Some(IntrospectValue::Text("0:0->2:0".to_owned())),
                "a restored edge keeps its stable id + endpoints",
            );

            assert!(stack.redo(), "redo the delete");
            assert_eq!(coord.node_count(), 3);
            assert_eq!(coord.edges.get().len(), 0);
        });
    }

    #[test]
    fn r851_connect_and_disconnect_round_trip_through_undo() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            // Disconnect seed edge 1 (1:0 -> 2:1), then undo restores it.
            assert!(coord.remove_edge(EdgeId(1)), "disconnect edge 1");
            assert_eq!(coord.edges.get().len(), 2);
            assert_eq!(stack.undo_label().as_deref(), Some("Disconnect"));
            assert!(stack.undo(), "undo the disconnect");
            assert_eq!(coord.edges.get().len(), 3, "the wire is back");
            assert_eq!(
                coord.query("edge.1"),
                Some(IntrospectValue::Text("1:0->2:1".to_owned())),
                "edge 1 is restored verbatim",
            );
            // Now re-make a connection and undo it.
            assert!(stack.redo(), "redo the disconnect");
            assert_eq!(coord.edges.get().len(), 2);
            let before = coord.edges.get().len();
            assert!(coord.add_edge(NodeId(1), 0, NodeId(2), 1), "reconnect 1:0 -> 2:1");
            assert_eq!(coord.edges.get().len(), before + 1);
            assert_eq!(stack.undo_label().as_deref(), Some("Connect"));
            assert!(stack.undo(), "undo the connect");
            assert_eq!(coord.edges.get().len(), before, "the new wire is gone");
        });
    }

    #[test]
    fn r851_connect_displacing_a_wire_undo_restores_the_displaced_wire() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            // Seed edge 0 = 0:0 -> 2:0. Connecting 1:0 -> 2:0 (single-wire input
            // rule) displaces edge 0; one undo restores it.
            assert!(coord.add_edge(NodeId(1), 0, NodeId(2), 0), "connect into an occupied input");
            assert_eq!(coord.edges.get().len(), 3, "one in, one out: count unchanged");
            assert_eq!(coord.query("edge.0"), None, "edge 0 was displaced");

            assert!(stack.undo(), "undo the displacing connect");
            assert_eq!(coord.edges.get().len(), 3);
            assert_eq!(
                coord.query("edge.0"),
                Some(IntrospectValue::Text("0:0->2:0".to_owned())),
                "the displaced wire is restored",
            );
        });
    }

    #[test]
    fn r851_a_new_edit_after_undo_truncates_the_redo_branch() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            let a = coord.add_node(0).expect("Texture");
            let b = coord.add_node(1).expect("Color");
            assert_eq!(coord.node_count(), 6);
            assert!(stack.undo(), "undo node b");
            assert!(stack.can_redo(), "b is redoable");
            assert!(coord.node_by_id(b).is_none());
            // A fresh add truncates the redo branch (single-branch QUndoStack).
            let c = coord.add_node(3).expect("Add");
            assert!(!stack.can_redo(), "the redo branch was dropped");
            assert_eq!(coord.node_count(), 6, "default 4 + a + c");
            assert!(coord.node_by_id(a).is_some() && coord.node_by_id(c).is_some());
            assert!(c.raw() > b.raw(), "ids stay monotonic across the truncation");
        });
    }

    #[test]
    fn r851_undo_external_query_and_invoke_round_trip() {
        Owner::new().run(|| {
            let mut scene = boot_full_scene();
            assert_eq!(undo_ext_query(&scene, "can_undo"), Some(IntrospectValue::Bool(false)));
            // Add a node through the primary coordinator.
            send(&mut scene, "palette_2:PointerDown");
            send(&mut scene, "palette_2:PointerUp");
            assert_eq!(query_int(&scene, "node_count"), 5);
            // The undo surface observes the history as data.
            assert_eq!(undo_ext_query(&scene, "can_undo"), Some(IntrospectValue::Bool(true)));
            assert_eq!(undo_ext_query(&scene, "index"), Some(IntrospectValue::Int(1)));
            assert_eq!(undo_ext_query(&scene, "count"), Some(IntrospectValue::Int(1)));
            assert_eq!(
                undo_ext_query(&scene, "undo_label"),
                Some(IntrospectValue::Text("Add Multiply".to_owned())),
            );
            // invoke undo on the external reverts the graph the coordinator reads.
            {
                let node = scene.find_external_with_tag_mut(UNDO_TAG).expect("undo external");
                let intro = node.handle.introspect_mut().expect("introspect");
                assert_eq!(intro.invoke("undo", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            }
            assert_eq!(query_int(&scene, "node_count"), 4, "RPC undo reverted the add");
            assert_eq!(undo_ext_query(&scene, "can_undo"), Some(IntrospectValue::Bool(false)));
            assert_eq!(undo_ext_query(&scene, "can_redo"), Some(IntrospectValue::Bool(true)));
        });
    }

    #[test]
    fn r851_ctrl_z_undoes_and_ctrl_shift_z_ctrl_y_redo() {
        Owner::new().run(|| {
            let mut scene = boot_full_scene();
            // Add a node, then Ctrl+Z removes it (the editor consumes the key).
            send(&mut scene, "palette_0:PointerDown");
            send(&mut scene, "palette_0:PointerUp");
            assert_eq!(query_int(&scene, "node_count"), 5);
            assert!(
                NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "z", mods(true, false)),
                "Ctrl+Z is handled",
            );
            assert_eq!(query_int(&scene, "node_count"), 4, "Ctrl+Z undid the add");
            // Ctrl+Y redoes.
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "y", mods(true, false)));
            assert_eq!(query_int(&scene, "node_count"), 5, "Ctrl+Y redid the add");
            // Ctrl+Shift+Z undoes again (the redo-pairing alternative is undo's twin).
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "z", mods(true, false)));
            assert_eq!(query_int(&scene, "node_count"), 4, "Ctrl+Z undid once more");
            assert!(
                NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "Z", mods(true, true)),
                "Ctrl+Shift+Z is handled",
            );
            assert_eq!(query_int(&scene, "node_count"), 5, "Ctrl+Shift+Z redid the add");
            // A plain 'z' (no Ctrl) is not an undo gesture — falls through.
            assert!(!NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "z", mods(false, false)));
        });
    }

    #[test]
    fn r851_undo_redo_at_boundaries_are_noops() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let stack = use_undo();
            assert!(!stack.undo(), "undo on empty history is a no-op");
            assert!(!stack.redo(), "redo on empty history is a no-op");
            assert_eq!(stack.len(), 0);
        });
    }

    // ── R852 serialization + persistence ───────────────────────────

    #[test]
    fn r852_serialized_query_is_json_with_schema_version_and_model() {
        Owner::new().run(|| {
            let coord = coordinator();
            let json = coord.serialized_json();
            let g: SerializedGraph = serde_json::from_str(&json).expect("valid JSON");
            assert_eq!(g.schema_version, PERSISTED_SCHEMA_VERSION);
            assert_eq!(g.nodes.len(), 4, "the seed nodes serialize");
            assert_eq!(g.edges.len(), 3, "the seed edges serialize");
            assert_eq!(g.next_node_id, first_dynamic_node_id(), "counters captured");
            assert_eq!(g.next_edge_id, first_dynamic_edge_id());
        });
    }

    #[test]
    fn r852_serialized_round_trips_through_set_graph() {
        Owner::new().run(|| {
            let coord = coordinator();
            // Snapshot the seed graph, mutate, then restore via set_graph.
            let snap = coord.serialized_json();
            coord.add_node(2);
            coord.add_node(0);
            assert_eq!(coord.node_count(), 6, "two nodes added");
            assert!(coord.load_json(&snap), "set_graph applies the snapshot");
            assert_eq!(coord.node_count(), 4, "the graph reverted to the snapshot");
            assert_eq!(coord.edges.get().len(), 3);
            assert_eq!(use_selection().get(), Selection::None, "selection dropped on load");
        });
    }

    #[test]
    fn r852_save_then_load_restores_the_graph_via_storage() {
        Owner::new().run(|| {
            let coord = coordinator();
            let a = coord.add_node(2).expect("Multiply"); // id 4
            assert_eq!(coord.node_count(), 5);
            assert!(coord.save(), "save the 5-node graph");
            // Mutate after the save.
            let b = coord.add_node(0).expect("Texture"); // id 5
            assert_eq!(coord.node_count(), 6);
            assert!(coord.load(), "load restores the saved graph");
            assert_eq!(coord.node_count(), 5, "back to the saved 5 nodes");
            assert!(coord.node_by_id(a).is_some(), "the saved node survives");
            assert!(coord.node_by_id(b).is_none(), "the post-save node is gone");
        });
    }

    #[test]
    fn r852_load_clears_the_undo_history() {
        Owner::new().run(|| {
            let coord = coordinator();
            let stack = use_undo();
            coord.add_node(2);
            assert!(stack.can_undo(), "the add is journaled");
            assert!(coord.save());
            coord.add_node(0);
            assert!(coord.load(), "load restores + clears undo");
            assert!(!stack.can_undo(), "the opened document is a fresh baseline");
            assert!(!stack.can_redo());
            assert_eq!(stack.len(), 0);
        });
    }

    #[test]
    fn r852_load_with_nothing_stored_is_a_noop() {
        Owner::new().run(|| {
            let coord = coordinator();
            assert!(!coord.load(), "nothing stored yet -> false");
            assert_eq!(coord.node_count(), 4, "the graph is unchanged");
        });
    }

    #[test]
    fn r852_set_graph_rejects_malformed_and_version_mismatch() {
        Owner::new().run(|| {
            let coord = coordinator();
            assert!(!coord.load_json("not json at all"), "malformed JSON rejected");
            assert_eq!(coord.node_count(), 4, "graph unchanged on malformed");
            // Valid JSON, wrong schema version.
            let bad = serde_json::to_string(&SerializedGraph {
                schema_version: PERSISTED_SCHEMA_VERSION + 1,
                nodes: Vec::new(),
                edges: Vec::new(),
                next_node_id: 0,
                next_edge_id: 0,
            })
            .unwrap();
            assert!(!coord.load_json(&bad), "version mismatch rejected");
            assert_eq!(coord.node_count(), 4, "graph unchanged on version mismatch");
        });
    }

    #[test]
    fn r852_loaded_counters_resume_monotonic_mint() {
        Owner::new().run(|| {
            let coord = coordinator();
            let a = coord.add_node(2).expect("Multiply"); // id 4, counter -> 5
            assert!(coord.save());
            let b = coord.add_node(0).expect("Texture"); // id 5, counter -> 6
            assert!(b.raw() > a.raw());
            assert!(coord.load(), "restore the counter to the saved value");
            // The next mint resumes at the saved counter: id b (the post-save
            // node) was discarded by the load, so reusing its number is correct
            // monotonic-from-the-saved-state, never an id live in the graph.
            let c = coord.add_node(0).expect("Texture");
            assert_eq!(c.raw(), a.raw() + 1, "next id resumes at the saved next_node_id");
        });
    }

    #[test]
    fn r852_save_load_set_graph_over_rpc_invoke() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // serialized query returns JSON.
            let json = match graph_intro(&scene).query("serialized") {
                Some(IntrospectValue::Text(s)) => s,
                other => panic!("expected serialized JSON, got {other:?}"),
            };
            assert!(json.contains("schema_version"), "serialized is the snapshot JSON");
            // Mutate, save, mutate again, load -> reverts.
            send(&mut scene, "palette_2:PointerDown");
            send(&mut scene, "palette_2:PointerUp");
            assert_eq!(query_int(&scene, "node_count"), 5);
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let intro = node.handle.introspect_mut().expect("introspect");
                assert_eq!(intro.invoke("save", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            }
            send(&mut scene, "palette_0:PointerDown");
            send(&mut scene, "palette_0:PointerUp");
            assert_eq!(query_int(&scene, "node_count"), 6);
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let intro = node.handle.introspect_mut().expect("introspect");
                assert_eq!(intro.invoke("load", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            }
            assert_eq!(query_int(&scene, "node_count"), 5, "RPC load reverted to the saved graph");
            // set_graph with the boot snapshot reverts all the way to 4 nodes.
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let intro = node.handle.introspect_mut().expect("introspect");
                assert_eq!(
                    intro.invoke("set_graph", IntrospectValue::Text(json)),
                    Ok(IntrospectValue::Bool(true)),
                );
            }
            assert_eq!(query_int(&scene, "node_count"), 4, "set_graph restored the boot snapshot");
            // A malformed set_graph is Rejected.
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let intro = node.handle.introspect_mut().expect("introspect");
                assert_eq!(
                    intro.invoke("set_graph", IntrospectValue::Text("garbage".to_owned())),
                    Err(InvokeError::Rejected),
                );
            }
        });
    }

    // ── R853 node move undo (drag = one step, nudge burst coalesces) ─

    /// Position of node `id` (panics if absent).
    fn pos_of(coord: &NodeGraphExternal, id: NodeId) -> (i32, i32) {
        coord.node_by_id(id).map(|n| (n.x, n.y)).expect("node present")
    }

    /// Arm + tear down a synthetic node-body drag from `before` to a `+delta`
    /// position (the real `pointer_move` rel-math is exercised by the demo's
    /// `tf.drag`; here we drive the latches directly to test the recording).
    fn synth_drag(coord: &NodeGraphExternal, id: NodeId, before: (i32, i32), dx: i32, dy: i32) {
        coord.grabbed_node.set(Some(id));
        coord.node_drag.set(Some(NodeDragStart {
            press_x_rel: 0.0,
            press_y_rel: 0.0,
            x_at_press: before.0,
            y_at_press: before.1,
        }));
        coord.set_node_pos(id, before.0 + dx, before.1 + dy); // live preview write
        coord.end_gesture(); // commits one non-coalescable move
    }

    #[test]
    fn r853_nudge_burst_coalesces_to_one_undo_step() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            let id = NodeId(0);
            coord.select_node(Some(id));
            let start = pos_of(&coord, id);
            assert!(coord.nudge_selected(NUDGE_STEP, 0));
            assert!(coord.nudge_selected(NUDGE_STEP, 0));
            assert!(coord.nudge_selected(NUDGE_STEP, 0));
            assert_eq!(stack.len(), 1, "the nudge burst is one coalesced undo step");
            assert_eq!(pos_of(&coord, id), (start.0 + 3 * NUDGE_STEP, start.1), "moved 3 steps");
            assert!(stack.undo(), "one undo reverts the whole burst");
            assert_eq!(pos_of(&coord, id), start, "back to the start");
            assert!(stack.redo(), "one redo re-applies the whole burst");
            assert_eq!(pos_of(&coord, id), (start.0 + 3 * NUDGE_STEP, start.1));
        });
    }

    #[test]
    fn r853_drag_records_one_move_at_gesture_end() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            let id = NodeId(0);
            let before = pos_of(&coord, id);
            coord.grabbed_node.set(Some(id));
            coord.node_drag.set(Some(NodeDragStart {
                press_x_rel: 0.0,
                press_y_rel: 0.0,
                x_at_press: before.0,
                y_at_press: before.1,
            }));
            coord.set_node_pos(id, before.0 + 50, before.1 + 30);
            assert_eq!(stack.len(), 0, "nothing is journaled mid-drag");
            coord.end_gesture();
            assert_eq!(stack.len(), 1, "the whole drag is one move at gesture end");
            assert!(stack.undo());
            assert_eq!(pos_of(&coord, id), before, "undo reverts the drag");
            assert!(stack.redo());
            assert_eq!(pos_of(&coord, id), (before.0 + 50, before.1 + 30), "redo re-applies it");
        });
    }

    #[test]
    fn r853_a_drag_does_not_coalesce_with_a_nudge() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            let id = NodeId(0);
            coord.select_node(Some(id));
            assert!(coord.nudge_selected(NUDGE_STEP, 0), "a coalescable nudge");
            let pos = pos_of(&coord, id);
            synth_drag(&coord, id, pos, 40, 0); // a non-coalescable drag
            assert_eq!(stack.len(), 2, "the drag is a fresh step, not folded into the nudge");
        });
    }

    #[test]
    fn r856_load_mid_drag_does_not_journal_a_spurious_move() {
        // R856 audit fix: a load / set_graph issued while a node drag is in
        // flight must clear the undo history AND not record the in-flight move
        // (apply_snapshot resets the gesture latches without journaling).
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            let snap = coord.serialized_json();
            let id = NodeId(0);
            let before = pos_of(&coord, id);
            // Arm an in-flight drag (grab + a live move), no release.
            coord.grabbed_node.set(Some(id));
            coord.node_drag.set(Some(NodeDragStart {
                press_x_rel: 0.0,
                press_y_rel: 0.0,
                x_at_press: before.0,
                y_at_press: before.1,
            }));
            coord.set_node_pos(id, before.0 + 40, before.1);
            assert!(coord.load_json(&snap), "load the snapshot mid-drag");
            assert!(!stack.can_undo(), "the opened document has a clean undo history");
            assert_eq!(stack.len(), 0, "no spurious MoveNodeCmd was journaled across the load");
        });
    }

    #[test]
    fn r856_non_contiguous_moves_do_not_coalesce() {
        // R856 audit fix: the merge contiguity guard. Two coalescable moves of
        // the same node whose positions do not chain (m1.after != m2.before)
        // stay two undo steps.
        Owner::new().run(|| {
            let _ = boot_scene();
            let stack = use_undo();
            let nodes = use_nodes();
            let id = NodeId(0);
            let cmd = |before, after| MoveNodeCmd {
                nodes: std::rc::Rc::clone(&nodes),
                id,
                before,
                after,
                coalescable: true,
            };
            stack.push_applied(cmd((0, 0), (100, 0)));
            stack.push_applied(cmd((200, 0), (300, 0))); // before != prior after
            assert_eq!(stack.len(), 2, "non-contiguous moves do not coalesce");
        });
    }

    #[test]
    fn r853_intervene_x_then_y_coalesce_to_one_move() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let stack = use_undo();
            let before_x = query_int(&scene, "node.0.x");
            let before_y = query_int(&scene, "node.0.y");
            {
                let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
                let intro = node.handle.introspect_mut().expect("introspect");
                intro.intervene("node.0.x", IntrospectValue::Int(200)).expect("x ok");
                intro.intervene("node.0.y", IntrospectValue::Int(150)).expect("y ok");
            }
            assert_eq!(stack.len(), 1, "x then y on the same node coalesce to one move");
            assert_eq!(query_int(&scene, "node.0.x"), 200);
            assert!(stack.undo(), "one undo reverts both axes");
            assert_eq!(query_int(&scene, "node.0.x"), before_x, "x restored");
            assert_eq!(query_int(&scene, "node.0.y"), before_y, "y restored");
        });
    }

    #[test]
    fn r853_move_after_add_is_a_separate_redoable_step() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let coord = coordinator();
            let stack = use_undo();
            let id = coord.add_node(0).expect("Texture"); // structural step
            let add_pos = pos_of(&coord, id);
            assert!(coord.nudge_selected(NUDGE_STEP, NUDGE_STEP), "move the new node");
            let moved_pos = pos_of(&coord, id);
            assert_ne!(add_pos, moved_pos);
            assert_eq!(stack.len(), 2, "add + move are two steps (move not folded into add)");
            assert!(stack.undo(), "undo the move");
            assert_eq!(pos_of(&coord, id), add_pos, "node back at its add-time position");
            assert!(stack.undo(), "undo the add");
            assert!(coord.node_by_id(id).is_none(), "the node is gone");
            assert!(stack.redo(), "redo the add");
            assert_eq!(pos_of(&coord, id), add_pos, "re-added at the add-time position");
            assert!(stack.redo(), "redo the move");
            assert_eq!(pos_of(&coord, id), moved_pos, "redo restores the moved position");
        });
    }

    #[test]
    fn r852_ctrl_s_saves_and_ctrl_o_loads() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            send(&mut scene, "palette_2:PointerDown");
            send(&mut scene, "palette_2:PointerUp");
            assert_eq!(query_int(&scene, "node_count"), 5);
            // Ctrl+S saves the 5-node graph.
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "s", mods(true, false)));
            // Add another, then Ctrl+O reverts to the saved graph.
            send(&mut scene, "palette_0:PointerDown");
            send(&mut scene, "palette_0:PointerUp");
            assert_eq!(query_int(&scene, "node_count"), 6);
            assert!(NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "o", mods(true, false)));
            assert_eq!(query_int(&scene, "node_count"), 5, "Ctrl+O loaded the saved graph");
            // Plain 's' (no Ctrl) is not a save gesture.
            assert!(!NodeEditorView::apply_key(&mut scene, Some(GRAPH_TAG), "s", mods(false, false)));
        });
    }
}
