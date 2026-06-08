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
//! `Signal<Vec<Edge>>` connection list, the `Signal<Option<usize>>` selection,
//! and a live-drag preview. It exposes the graph for AI-first introspection:
//! `query node_count` / `edge_count` / `node.<i>.{title,x,y,inputs,outputs}` /
//! `edge.<i>` / `selected`; `intervene node.<i>.x` / `node.<i>.y` / `selected`;
//! `invoke add_edge` / `remove_edge` / `delete_node` / `delete_selected` /
//! `nudge` / the pointer `send` wire.
//!
//! ## Keyboard (single Tab stop, the graph)
//!
//! Arrow keys nudge the selected node; `Delete` / `Backspace` removes it (and
//! its incident edges); `Escape` clears the selection.
//!
//! ## a11y (R838 §5.40)
//!
//! The graph lowers to a WAI-ARIA `list` of `listitem`s — one per node, named
//! `"<title> (<n> in, <m> out)"`, the selected node flagged. A true
//! diagram / `graphics-document` role tree (per-port, per-edge nodes) is a
//! follow-up once a 2nd consumer fixes the shape ([[abstraction-needs-second-consumer]]).
//!
//! ## Known gaps (honest carry)
//!
//! - Edge geometry lift: the bezier-between-ports helper is example-local; the
//!   2nd consumer (a self-hosted material graph) lifts it to
//!   `pinion_widget_paint` (the chip / stepper inline-first precedent).
//! - Edge selection / click-to-delete: edges are created by port drag + RPC
//!   and removed by index — bezier hit-test is a follow-up.
//! - Node rename (inline editor), canvas pan / zoom, multi-select marquee,
//!   stable node ids (delete reindexes) — all additive follow-ups.

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{
    int_of, Backend, BackendFallback, BackendSupport, CaptureNormalize, DragPayload, DropPoint,
    External, ExternalIntrospect, IntrospectSchema, IntrospectValue, InterveneError, InvokeError,
    RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{batch, Owner, Signal};
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
const PREVIEW_W: u32 = 2;
const NUDGE_STEP: i32 = 12;

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

/// One node: a titled card with `inputs` input ports (left edge) and
/// `outputs` output ports (right edge), placed at canvas `(x, y)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct GraphNode {
    title: String,
    x: i32,
    y: i32,
    inputs: usize,
    outputs: usize,
}

impl GraphNode {
    fn new(title: &str, x: i32, y: i32, inputs: usize, outputs: usize) -> Self {
        Self { title: title.to_owned(), x, y, inputs, outputs }
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

/// A directed connection: source node's output port → target node's input
/// port. Node indices are positions in the model `Vec` (delete reindexes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Edge {
    from_node: usize,
    from_port: usize,
    to_node: usize,
    to_port: usize,
}

/// Live drag preview while a wire is being pulled from an output port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Preview {
    from_node: usize,
    from_port: usize,
    /// Currently-hovered input port the wire would snap to, if any.
    to: Option<(usize, usize)>,
}

/// First-paint graph — a tiny material graph (`Texture` × `Color` →
/// `Multiply` → `Output`).
fn default_nodes() -> Vec<GraphNode> {
    vec![
        GraphNode::new("Texture", 40, 70, 0, 1),
        GraphNode::new("Color", 40, 210, 0, 1),
        GraphNode::new("Multiply", 250, 110, 2, 1),
        GraphNode::new("Output", 470, 150, 1, 0),
    ]
}

fn default_edges() -> Vec<Edge> {
    vec![
        Edge { from_node: 0, from_port: 0, to_node: 2, to_port: 0 },
        Edge { from_node: 1, from_port: 0, to_node: 2, to_port: 1 },
        Edge { from_node: 2, from_port: 0, to_node: 3, to_port: 0 },
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
fn use_selected() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_selected requires an active Owner scope");
    owner.cache("node_graph.selected", || Signal::new(None))
}

#[must_use]
fn use_preview() -> Rc<Signal<Option<Preview>>> {
    let owner = Owner::current().expect("use_preview requires an active Owner scope");
    owner.cache("node_graph.preview", || Signal::new(None))
}

// ─── sub-tag grammar (composite paint tags route to the coordinator) ──

/// `node_graph#node_<i>` → node index.
fn parse_node_sub(sub: &str) -> Option<usize> {
    sub.strip_prefix("node_")?.parse().ok()
}

/// `oport_<n>_<j>` → (node, output port).
fn parse_oport_sub(sub: &str) -> Option<(usize, usize)> {
    let (n, j) = sub.strip_prefix("oport_")?.split_once('_')?;
    Some((n.parse().ok()?, j.parse().ok()?))
}

/// A full drop tag `node_graph#iport_<n>_<i>` → (node, input port).
fn parse_input_port_tag(tag: &str) -> Option<(usize, usize)> {
    let (_, sub) = tag.split_once('#')?;
    let (n, i) = sub.strip_prefix("iport_")?.split_once('_')?;
    Some((n.parse().ok()?, i.parse().ok()?))
}

/// Two comma-separated `usize`s ("dx,dy" sign-prefixed allowed via i32).
fn parse_pair_i32(s: &str) -> Option<(i32, i32)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Four comma-separated `usize`s ("from_node,from_port,to_node,to_port").
fn parse_quad(csv: &str) -> Option<(usize, usize, usize, usize)> {
    let parts: Vec<&str> = csv.split(',').collect();
    let [fnode, fport, tnode, tport] = parts.as_slice() else {
        return None;
    };
    Some((
        fnode.trim().parse().ok()?,
        fport.trim().parse().ok()?,
        tnode.trim().parse().ok()?,
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
    None,
    NodeBody,
    OutputPort(usize, usize),
}

// ─── coordinator External ──────────────────────────────────────────

/// The node-graph coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) plus the internal drag latches.
struct NodeGraphExternal {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    edges: Rc<Signal<Vec<Edge>>>,
    selected: Rc<Signal<Option<usize>>>,
    preview: Rc<Signal<Option<Preview>>>,
    /// Node grabbed for a capture-drag move (set on a node-body `PointerDown`).
    grabbed_node: Cell<Option<usize>>,
    node_drag: Cell<Option<NodeDragStart>>,
    pending_press: Cell<PendingPress>,
}

impl NodeGraphExternal {
    fn new(
        nodes: Rc<Signal<Vec<GraphNode>>>,
        edges: Rc<Signal<Vec<Edge>>>,
        selected: Rc<Signal<Option<usize>>>,
        preview: Rc<Signal<Option<Preview>>>,
    ) -> Self {
        Self {
            nodes,
            edges,
            selected,
            preview,
            grabbed_node: Cell::new(None),
            node_drag: Cell::new(None),
            pending_press: Cell::new(PendingPress::None),
        }
    }

    fn node_count(&self) -> usize {
        self.nodes.get().len()
    }

    /// Move node `i` to a clamped `(x, y)`. The single mutation behind both
    /// the capture drag and the `intervene node.<i>.{x,y}` path.
    fn set_node_pos(&self, i: usize, x: i32, y: i32) -> bool {
        let mut moved = false;
        self.nodes.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(node) = next.get_mut(i) {
                node.x = clamp_node_x(x);
                node.y = clamp_node_y(y);
                moved = true;
            }
            next
        });
        moved
    }

    /// Add an edge output `(from_node, from_port)` → input `(to_node,
    /// to_port)`. Rejects a self-loop, an out-of-range port, or a duplicate;
    /// an input port takes a single wire, so an existing connection into the
    /// target input is replaced (the canonical node-editor rule).
    fn add_edge(&self, from_node: usize, from_port: usize, to_node: usize, to_port: usize) -> bool {
        if from_node == to_node {
            return false;
        }
        let nodes = self.nodes.get();
        let Some(src) = nodes.get(from_node) else {
            return false;
        };
        let Some(dst) = nodes.get(to_node) else {
            return false;
        };
        if from_port >= src.outputs || to_port >= dst.inputs {
            return false;
        }
        let edge = Edge { from_node, from_port, to_node, to_port };
        let mut edges = self.edges.get();
        if edges.contains(&edge) {
            return false;
        }
        edges.retain(|e| !(e.to_node == to_node && e.to_port == to_port));
        edges.push(edge);
        self.edges.set(edges);
        true
    }

    fn remove_edge(&self, index: usize) -> bool {
        let mut edges = self.edges.get();
        if index >= edges.len() {
            return false;
        }
        edges.remove(index);
        self.edges.set(edges);
        true
    }

    /// Delete node `k`, drop its incident edges, and reindex the survivors
    /// (node indices are model positions). Selection follows the same shift.
    fn delete_node(&self, k: usize) -> bool {
        if k >= self.node_count() {
            return false;
        }
        batch(|| {
            self.nodes.set_with(|prev| {
                let mut next = prev.clone();
                next.remove(k);
                next
            });
            self.edges.set_with(|prev| {
                let mut next: Vec<Edge> =
                    prev.iter().copied().filter(|e| e.from_node != k && e.to_node != k).collect();
                for e in &mut next {
                    if e.from_node > k {
                        e.from_node -= 1;
                    }
                    if e.to_node > k {
                        e.to_node -= 1;
                    }
                }
                next
            });
            self.selected.set(match self.selected.get() {
                Some(s) if s == k => None,
                Some(s) if s > k => Some(s - 1),
                other => other,
            });
        });
        self.grabbed_node.set(None);
        self.node_drag.set(None);
        true
    }

    fn delete_selected(&self) -> bool {
        match self.selected.get() {
            Some(k) => self.delete_node(k),
            None => false,
        }
    }

    /// Nudge the selected node by `(dx, dy)` (the arrow-key path).
    fn nudge_selected(&self, dx: i32, dy: i32) -> bool {
        let Some(k) = self.selected.get() else {
            return false;
        };
        let nodes = self.nodes.get();
        let Some(node) = nodes.get(k) else {
            return false;
        };
        self.set_node_pos(k, node.x + dx, node.y + dy)
    }

    /// Set / clear the selection, clamping an in-range index.
    fn set_selected(&self, value: Option<usize>) {
        let clamped = value.filter(|&i| i < self.node_count());
        self.selected.set(clamped);
    }

    /// Pointer `send` wire (the same channel the router and RPC share).
    fn handle_send(&mut self, payload: &str) -> IntrospectValue {
        let parts: Vec<&str> = payload.split(':').collect();
        let (sub, event) = match parts.as_slice() {
            [event] => (None, *event),
            [sub, event, ..] => (Some(*sub), *event),
            [] => (None, ""),
        };
        match (sub, event) {
            (Some(s), "PointerDown") => {
                if parse_node_sub(s).is_some() {
                    self.grabbed_node.set(parse_node_sub(s));
                    self.node_drag.set(None);
                    self.pending_press.set(PendingPress::NodeBody);
                } else if let Some((n, j)) = parse_oport_sub(s) {
                    self.pending_press.set(PendingPress::OutputPort(n, j));
                } else {
                    self.pending_press.set(PendingPress::None);
                }
            }
            (Some(s), "PointerUp") => {
                if let Some(n) = parse_node_sub(s) {
                    self.set_selected(Some(n));
                }
                self.end_gesture();
            }
            (None, "PointerUp") => {
                // Background release — deselect (only when no drag was armed).
                if self.pending_press.get() == PendingPress::None && self.grabbed_node.get().is_none()
                {
                    self.selected.set(None);
                }
                self.end_gesture();
            }
            (None, "PointerDown") => {
                self.pending_press.set(PendingPress::None);
            }
            _ => {}
        }
        IntrospectValue::Null
    }

    fn end_gesture(&self) {
        self.grabbed_node.set(None);
        self.node_drag.set(None);
        self.pending_press.set(PendingPress::None);
    }
}

impl core::fmt::Debug for NodeGraphExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NodeGraphExternal")
            .field("nodes", &self.node_count())
            .field("edges", &self.edges.get().len())
            .field("selected", &self.selected.get())
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
            return;
        };
        match self.node_drag.get() {
            None => {
                let nodes = self.nodes.get();
                if let Some(n) = nodes.get(node) {
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
            if let Some((from_node, from_port)) =
                src.split_once('_').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
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
            ("selected", "int"),
            ("node.<i>.title", "string"),
            ("node.<i>.x", "int"),
            ("node.<i>.y", "int"),
            ("node.<i>.inputs", "int"),
            ("node.<i>.outputs", "int"),
            ("edge.<i>", "string"),
            ("send", "string"),
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
            "selected" => Some(match self.selected.get() {
                Some(i) => IntrospectValue::Int(int_of(i)),
                None => IntrospectValue::Null,
            }),
            _ => {
                if let Some(rest) = path.strip_prefix("node.") {
                    let (i_str, field) = rest.split_once('.')?;
                    let i: usize = i_str.parse().ok()?;
                    let nodes = self.nodes.get();
                    let node = nodes.get(i)?;
                    return match field {
                        "title" => Some(IntrospectValue::Text(node.title.clone())),
                        "x" => Some(IntrospectValue::Int(i64::from(node.x))),
                        "y" => Some(IntrospectValue::Int(i64::from(node.y))),
                        "inputs" => Some(IntrospectValue::Int(int_of(node.inputs))),
                        "outputs" => Some(IntrospectValue::Int(int_of(node.outputs))),
                        _ => None,
                    };
                }
                if let Some(i_str) = path.strip_prefix("edge.") {
                    let i: usize = i_str.parse().ok()?;
                    let edges = self.edges.get();
                    let e = edges.get(i)?;
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
                    self.set_selected(None);
                    Ok(())
                }
                IntrospectValue::Int(i) => {
                    let idx = usize::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_selected(Some(idx));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            };
        }
        let Some(rest) = path.strip_prefix("node.") else {
            return Err(InterveneError::UnknownPath);
        };
        let (i_str, field) = rest.split_once('.').ok_or(InterveneError::UnknownPath)?;
        let i: usize = i_str.parse().map_err(|_| InterveneError::UnknownPath)?;
        if i >= self.node_count() {
            return Err(InterveneError::UnknownPath);
        }
        // The field decides read-only-ness first (a read-only field rejects
        // any value type), then `x` / `y` require an `Int`.
        match field {
            "x" | "y" => {
                let IntrospectValue::Int(v) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let coord = i32::try_from(v).map_err(|_| InterveneError::TypeMismatch)?;
                let node = self.nodes.get()[i].clone();
                if field == "x" {
                    self.set_node_pos(i, coord, node.y);
                } else {
                    self.set_node_pos(i, node.x, coord);
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
                    let idx = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.remove_edge(idx)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "delete_node" => match args {
                IntrospectValue::Int(i) => {
                    let idx = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.delete_node(idx)))
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
    let ctrl = (to.0 - from.0).abs().max(60) / 2;
    let commands = vec![
        PathCommand::MoveTo(ppt(from.0, from.1)),
        PathCommand::CurveTo {
            c1: ppt(from.0 + ctrl, from.1),
            c2: ppt(to.0 - ctrl, to.1),
            end: ppt(to.0, to.1),
        },
    ];
    // Tight bounding box at the wire's extent — and `pointer_transparent`
    // so the (decorative) edge never intercepts a click meant for the
    // canvas background or a node card beneath it.
    let (ox, oy) = (from.0.min(to.0), from.1.min(to.1));
    let (bw, bh) = ((from.0 - to.0).abs().max(1), (from.1 - to.1).abs().max(1));
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

/// All committed edges, resolved to their port centres. Painted behind the
/// node cards.
fn view_edges(nodes: &[GraphNode], edges: &[Edge], theme: &Theme) -> Vec<Scene> {
    let color = theme.resolve(ColorRole::Accent);
    edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let from = output_port_center(nodes.get(e.from_node)?, e.from_port);
            let to = input_port_center(nodes.get(e.to_node)?, e.to_port);
            Some(view_edge(format!("{GRAPH_TAG}#edge_{i}"), from, to, color, EDGE_W))
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
fn view_node(idx: usize, node: &GraphNode, selected: bool, theme: &Theme) -> Scene {
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
            format!("{GRAPH_TAG}#iport_{idx}_{i}"),
            0,
            port_row_top(i),
            port_color,
        ));
    }
    for j in 0..node.outputs {
        children.push(view_port(
            format!("{GRAPH_TAG}#oport_{idx}_{j}"),
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
            .with_tag(format!("{GRAPH_TAG}#node_{idx}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(border))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(node.x), upx(node.y))
                    .with_size(Size::px(upx(NODE_W), upx(node.height()))),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let nodes = use_nodes().get();
    let edges = use_edges().get();
    let selected = use_selected().get();
    let preview = use_preview().get();

    let mut children: Vec<Scene> = Vec::new();

    // Edges (behind) → preview wire → node cards (on top) → chrome.
    children.extend(view_edges(&nodes, &edges, &theme));

    if let Some(p) = preview {
        if let Some(from_node) = nodes.get(p.from_node) {
            let from = output_port_center(from_node, p.from_port);
            let to = p
                .to
                .and_then(|(tn, tp)| Some(input_port_center(nodes.get(tn)?, tp)))
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

    for (idx, node) in nodes.iter().enumerate() {
        children.push(view_node(idx, node, selected == Some(idx), &theme));
    }

    children.push(Scene::Text(
        TextNode::styled(
            "Node graph",
            Rect::default(),
            TextStyle::new().with_size_px(TITLE_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, 12)),
    ));

    let status = match selected {
        Some(i) => format!(
            "{} nodes · {} edges · selected: {}",
            nodes.len(),
            edges.len(),
            nodes.get(i).map_or("—", |n| n.title.as_str())
        ),
        None => format!("{} nodes · {} edges · none selected", nodes.len(), edges.len()),
    };
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

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(GRAPH_TAG)
            .with_aria_label("Node graph")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

// ─── WidgetCore impl ───────────────────────────────────────────────

struct NodeEditorView;

impl WidgetCore for NodeEditorView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(NodeGraphExternal::new(use_nodes(), use_edges(), use_selected(), use_preview()))
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
    /// R838 §5.40 — the graph lowers to a WAI-ARIA `list` whose `listitem`
    /// children are the nodes (named by title + port arity, selection
    /// flagged). A true diagram role tree is a follow-up.
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        let nodes = use_nodes().get();
        let selected = use_selected().get();
        let mut list = AccessNode::new(GRAPH_TAG, AriaRole::List)
            .with_name("Node graph")
            .with_state(AccessState { focused: focused == Some(GRAPH_TAG), ..AccessState::default() });
        for i in 0..nodes.len() {
            list = list.with_child(format!("{GRAPH_TAG}#node_{i}"));
        }
        let mut out = vec![list];
        for (i, node) in nodes.iter().enumerate() {
            out.push(
                AccessNode::new(format!("{GRAPH_TAG}#node_{i}"), AriaRole::ListItem)
                    .with_name(format!(
                        "{} ({} in, {} out)",
                        node.title, node.inputs, node.outputs
                    ))
                    .with_set_position(i, nodes.len())
                    .with_selected(selected == Some(i)),
            );
        }
        out
    }
}

impl WidgetView for NodeEditorView {
    type Renderer = HelloNodeEditorRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
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
            // A new valid edge into Output's only input replaces edge 2's target.
            assert_eq!(
                intro.invoke("add_edge", IntrospectValue::Text("0,0,3,0".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            // Input (3,0) now has exactly one wire — still 3 edges total.
            assert_eq!(intro.query("edge_count"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("edge.2"), Some(IntrospectValue::Text("0:0->3:0".to_owned())));
        });
    }

    #[test]
    fn r838_remove_edge_by_index() {
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
    fn r838_delete_node_reindexes_edges_and_selection() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRAPH_TAG).expect("present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("selected", IntrospectValue::Int(3));
            // Delete node 1 (Color): drops edge 1:0->2:1, and node 2/3 shift to 1/2.
            assert_eq!(
                intro.invoke("delete_node", IntrospectValue::Int(1)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("node_count"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("edge_count"), Some(IntrospectValue::Int(2)));
            // Old node 2 (Multiply) is now node 1; old edge 0:0->2:0 became 0:0->1:0.
            assert_eq!(intro.query("node.1.title"), Some(IntrospectValue::Text("Multiply".to_owned())));
            assert_eq!(intro.query("edge.0"), Some(IntrospectValue::Text("0:0->1:0".to_owned())));
            // Selection (was 3 = Output) follows the shift to 2.
            assert_eq!(intro.query("selected"), Some(IntrospectValue::Int(2)));
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
    fn r838_access_node_emits_list_with_selected_item() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_selected().set(Some(2));
            let nodes = NodeEditorView::access_node(&(), Some(GRAPH_TAG));
            assert_eq!(nodes.len(), 1 + 4, "list + one item per node");
            assert_eq!(nodes[0].role, AriaRole::List);
            assert!(nodes[0].state.focused);
            let multiply = nodes
                .iter()
                .find(|n| n.tag == format!("{GRAPH_TAG}#node_2"))
                .expect("Multiply node present");
            assert_eq!(multiply.role, AriaRole::ListItem);
            assert_eq!(multiply.name.as_deref(), Some("Multiply (2 in, 1 out)"));
            assert_eq!(multiply.selected, Some(true));
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
