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
//! * **Edge connect / reconnect** — the R742 drag substrate
//!   ([`External::begin_drag`] from an output port →
//!   [`External::drag_release`] over an input port). `drag_release`'s `over`
//!   carries the dropped-on tag, so the target input port falls straight out
//!   of the router's hit-test — no per-binding drop resolver. R1174 — grabbing
//!   a *wired* input port instead arms a **reconnect** drag through the same two
//!   methods (the loose end is the existing edge's source output), committing
//!   via `reconnect_edge`.
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
//! `Signal<Vec<Edge>>` connection list, a single [`Selection`] sum type (node
//! set | edge | none — `Signal<Selection>`; R879 generalised the node arm to a
//! non-empty `BTreeSet`: plain click replaces, `Ctrl`+click toggles,
//! `Shift`+click adds, and grabbing a selected node drags the whole group
//! rigidly), and a live-drag preview. Nodes and edges
//! carry stable [`NodeId`] / [`EdgeId`] handles (R841): addressing is by id, so
//! deleting one entity never renumbers the survivors. It exposes the graph for
//! AI-first introspection: `query node_count` / `edge_count` / `node_ids` /
//! `edge_ids` / `reroute_ids` (R1242) / `node.<id>.{title,x,y,h,inputs,outputs,is_reroute,op,is_source,input_types,output_types,input_default.<port>,value,resolved_input.<port>}` (R1256: `op` = rename-stable compute identity; R1257: `is_source` + authorable `value` on sources; R1260: `resolved_input.<port>` = the value that actually flows in, a debugger read) /
//! `edge.<id>` / `dissolvable.<id>` / `dissolvable_ids` (R1241: dissolve
//! eligibility) / `eval.{output,acyclic,cycle_nodes}` (R1255: terminal value +
//! DAG check; R1260: `cycle_nodes` localises a cycle) / `selected` /
//! `selected_ids` / `selected_edge`;
//! `intervene node.<id>.x` / `node.<id>.y` / `node.<id>.title` /
//! `node.<id>.input_default.<port>` / `frame.<id>.{title,x,y,w,h}` (R1234:
//! `x`/`y` move the frame + its contents, `w`/`h` resize it) /
//! `selected` / `selected_ids` /
//! `selected_edge`; `invoke add_edge` / `remove_edge` /
//! `reconnect_edge` / `add_reroute` (R1235: splice a typed passthrough into an
//! edge; R1243: also the live **double-click-on-wire** gesture, and the knot
//! paints as a compact [`KNOT_SIZE`] dot) / `delete_node` / `delete_selected` /
//! `dissolve_node` /
//! `dissolve_selected` (R1236: delete + reconnect through a 1-in/1-out node) /
//! `nudge` / the pointer `send` wire.
//!
//! R929 / R1174 — **reconnect** re-wires an existing edge's *target* input
//! (keeping its source output): the canonical "grab a wired input and drop it
//! elsewhere" graph edit, as one atomic [`UndoStack`] step (remove old + add new
//! in a single `Ctrl+Z`), validated through the same
//! [`NodeGraphExternal::validate_connection`] SSOT as `add_edge` (self-loop /
//! typed-port / single-wire-into-input). Both halves now share that one verb:
//! the §2 AI-first path is `invoke reconnect_edge "edge,to_node,to_port"`
//! (R929), and the human-editor **gesture** is grabbing a wired input port and
//! dragging it loose onto another input (R1174). The gesture is the small R742
//! drag-substrate extension R929 flagged — an input-port [`PendingPress`] arm
//! that reuses `begin_drag` / `drag_release`, with the grabbed edge id riding in
//! the [`Preview`] so the loose end commits through `reconnect_edge`. So, like
//! node move and edge connect, reconnect is now "a live drag *and* the AI path
//! are one source of truth"; R929's honest verb-first gap is closed.
//!
//! ## Keyboard (single Tab stop, the graph)
//!
//! Arrow keys nudge the selected node **set** (one undo step per burst);
//! `Delete` / `Backspace` removes the selection (the node set + every
//! incident edge as one undo step, or an edge); `Escape` clears it.
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
//! - **Typed ports** (R898): ports are [`PortType`]-typed sockets, not bare
//!   counts. `add_edge` rejects an ill-typed wire — the output's type must be
//!   assignable to the input's (exact, or a `Float`->`Vector` scalar
//!   broadcast). Ports paint in their type's signature colour (the
//!   colour-coded-pin convention), and the types are AI-readable
//!   (`query node.<id>.{input_types,output_types}`). Two material-graph types
//!   for now (`Float`, `Vector`). **Honest status (R1259)**: this is a
//!   *single-consumer prototype* — the type lattice, the `NodeOp` taxonomy, AND
//!   the eval/validation *mechanism* all live in THIS example binary; nothing
//!   external depends on them, so none of it is yet the reusable substrate the
//!   self-hosted editor (true north) would dogfood. `#[non_exhaustive]` is a
//!   *forward-looking* marker: it constrains only a future downstream crate, so
//!   it is inert within this module today. When a 2nd consumer materialises
//!   ([[abstraction-needs-second-consumer]]) the lift is a **redesign** into a
//!   `pinion-node-graph`-style crate with **trait/registry-dispatched** eval
//!   (the taxonomy becoming the consumer's, the mechanism genuinely shared) —
//!   NOT a move of today's closed `match`.
//! - **Port default values** (R899): each input port carries a typed literal
//!   default (the "pin default value"), typed by its [`PortType`] — a `Float`
//!   port a scalar, a `Vector` port a colour — reusing the data-grid
//!   [`CellValue`] value substrate. An **unconnected** port paints its default
//!   beside the pin (a wired port hides it; the value is retained); it is
//!   AI-read/write (`query`/`intervene node.<id>.input_default.<port>`, typed —
//!   a colour takes a `#RRGGBB[AA]` hex, the write journals an undoable
//!   `GraphEdit`). The painted default is **inline-editable on the
//!   canvas** (R901): a **double-click** on the default opens the shared inline
//!   field (the same affordance the title rename uses, R878), while the input
//!   port's *single*-click stays edge-connect (R742); the AI path is the
//!   `intervene node.<id>.input_default.<port>` write above.
//! - **Dataflow evaluation** (R1255 — the Phase-C entry): the authored graph is
//!   now *computed*, not only edited. Each node carries a first-class
//!   [`MaterialOp`] compute identity (the SSOT the evaluator dispatches on — NOT the
//!   rewritable title, the R1242 identity lesson generalised); a topological,
//!   cycle-safe, memoised pass resolves each input (a wired source's output,
//!   else its R899 pin default), coerces it to the port type (the `Float →
//!   Vector` scalar-broadcast the type lattice permits), and applies the op —
//!   a tiny material-graph vocabulary (`Add`/`Multiply`/`Lerp` colour ops,
//!   `Texture`/`Color`/`Scalar` sources, the `Output` sink). It is pure derived
//!   introspection for the derived reads (§2 #3-friendly, no mutation): `query
//!   node.<id>.value` reads a node's output (a `Float` float / a `Vector`
//!   `{hex,r,g,b,a}` object / `null` on a cycle), `query eval.output` the
//!   terminal value at the `Output` sink, `query eval.acyclic` the DAG check.
//!   Authoring drives the result: `intervene`-ing a pin default or wiring a
//!   source re-computes the reads. **R1257** — a *source* node (`Texture` /
//!   `Color` / `Scalar`) now carries an **authorable output constant** (the
//!   output-side twin of the R899 pin default): `intervene node.<id>.value`
//!   authors it (`node.<id>.is_source` flags which nodes accept the write; a
//!   compute op / sink rejects it `ReadOnly`), and the graph re-evaluates.
//!   **Still deferred**: painting the source constant on its card + an inline
//!   editor (this round is the AI-first substrate — the GUI authoring affordance
//!   is a follow-up, the R899→R901 split), multi-output ops, and the typed
//!   ports' AT enrichment (the a11y name keeps the arity count: `pinion-a11y`
//!   has no diagram / `graphics-document` role yet — an upstream substrate gap,
//!   the same one the module-level a11y note records).
//! - **Undo / redo** (R851 + R853): every edit is reversible on the shared
//!   [`UndoStack`] — the **structural** edits (add node, delete node + its
//!   incident edges, connect, disconnect) as [`GraphEdit`] deltas, and node
//!   **moves** (drag / nudge / `intervene .x`) as `GraphEdit`s. A drag is
//!   recorded as one move at gesture end; a keyboard nudge **burst** coalesces
//!   to one undo step (the `UndoCommand::merge` hook). `Ctrl+Z` /
//!   `Ctrl+Shift+Z` (`Ctrl+Y`) and the AI-first [`UndoStackExternal`] drive it.
//! - **Persistence** (R852, R857): the graph saves / loads through the §3 §5.15
//!   [`Storage`] substrate — `save` / `load` write and restore a
//!   [`SerializedGraph`] JSON blob (nodes + edges + the monotonic id counters)
//!   via [`pinion_platform_storage::use_app_storage`] (a `FileStorage` when the
//!   platform offers a data dir, the in-memory fallback otherwise — R857 lifted
//!   this runtime-selection hook out of the per-example copies, so the binding
//!   really reaches the file backend). The same snapshot is the AI-first
//!   `query serialized` / `invoke set_graph` read-write pair, and `Ctrl+S` /
//!   `Ctrl+O` drive save / open. Loading a graph clears the undo history (the
//!   opened document is a fresh baseline — the undo stack model). R1258 — an
//!   `set_graph` blob is **structurally validated** ([`graph_invariants_hold`])
//!   before it is installed, so an untrusted AI-first write of a malformed graph
//!   (ill-typed edge / wrong-arity op / duplicate id / mistyped default) is
//!   rejected LOUD (the graph unchanged) rather than silently mis-evaluated.
//! - ~~`add_node` over RPC~~ — landed R849 (the palette sidebar + the
//!   `add_node` invoke verb; this bullet was stale until the R878 audit).
//! - **Crate extraction**: the model + pure bezier geometry are example-local;
//!   the 2nd consumer (a self-hosted material graph) lifts them to a crate
//!   (the chip / stepper inline-first precedent) — extract, not extend.
//! - **Canvas pan / zoom** (R877): pan = a [`ScrollAxis::Both`] scroll over a
//!   fixed `WORLD`-extent surface (the finite huge canvas — plain wheel pans
//!   through the router's native scroll dispatch, zero canvas code);
//!   zoom = a shared `Signal<f64>` the view projects every world coordinate
//!   through. `Ctrl`+wheel zooms anchored at the cursor (the R877
//!   [`External::wheel`] forwarding leg), `Shift`+wheel pans horizontally,
//!   `Ctrl`+`=`/`-`/`0` step/reset, `f` frames the graph. AI-first:
//!   `query`/`intervene viewport.{x,y,zoom}` (pan in zoom-independent graph
//!   units) + `invoke frame_all`. Drag-to-pan already works through the shared
//!   R881 / R882 router pan channel (middle-drag + `Space`+left-drag over the
//!   world `ScrollNode`; zero canvas code). **Edge-drag auto-pan** (R1182): a
//!   node held against the canvas rim auto-scrolls the viewport toward that
//!   edge every frame (the [`AutoPan`] [`Tickable`], [`use_autopan`]) and the
//!   dragged nodes stay pinned under the cursor — the DCC / the engine
//!   drag-past-the-edge convention. Wire-connect auto-pan (the `begin_drag`
//!   DnD path) stays a documented follow-up.
//! - **Inline edit** (R878 title / R901 port default): double-click a node
//!   card (or `F2` on the selection, or `invoke begin_rename`) edits the
//!   title; double-click a pin's default label (or `invoke
//!   begin_edit_default`) edits that input port's literal default. Both open
//!   ONE shared inline `TextFieldExternal` (the R790 todomvc `EDIT_TF`
//!   modal-member shape) keyed by an [`EditTarget`]. `Enter` commits,
//!   `Escape` cancels, a click-away commits through the R793 blur intent; the
//!   keymap is the lifted [`pinion_core::edit_field_keymap`] SSOT with the
//!   target's typed [`CellKind`] gate (title = text, a `Float` port = a
//!   number, a `Vector`/`Color` port = a `#RRGGBB[AA]` hex). A commit journals
//!   an undoable `GraphEdit` / `GraphEdit` through the
//!   `apply_rename` / `apply_set_node_value` SSOT — the same path the AI-first
//!   `intervene node.<id>.title` / `node.<id>.input_default.<port>` (and R1257
//!   `node.<id>.value` on a source) write-twins drive (`query editing` is the
//!   in-flight read; `query renaming` survives as its title-only projection).
//! - **Double-click a wire** (R1243): splices a reroute knot into the wire under
//!   the cursor (`A -> R -> B`), the live gesture twin of `invoke add_reroute
//!   <edge_id>`. Reads the background edge-hit probe the same in-place click
//!   selects a wire from, so a double-click on empty canvas no-ops.
//! - **Double-click a reroute knot** (R1246): a no-op — a knot paints no card,
//!   so `begin_edit` refuses the card-title rename (the paint==a11y gate). To
//!   remove a knot, use `Alt`+`Delete` / `invoke dissolve_node` (R1236), which
//!   reconnects the wire through it. (R1245's double-click-to-dissolve was
//!   reverted: a destructive action on a benign gesture is a footgun that
//!   duplicated the standard delete.)
//! - **Multi-select marquee** (R879 model / R880 gesture) — an LMB background-
//!   drag rubber-bands a rect (`MarqueeStart` + the `DragLatch` click-vs-drag
//!   dead zone); on release every intersected node joins the set via
//!   `apply_marquee`, with `Ctrl`/`Shift` extending. The selection model is
//!   `Selection::Nodes` + `selected_ids`.
//! - **Align / distribute** (R948): `invoke align_{left,center_h,right,top,
//!   center_v,bottom}` snaps the selected nodes to one edge / centre of their
//!   bounding box; `invoke distribute_{h,v}` spaces their centres evenly
//!   (extremes fixed). Each is ONE non-coalescing undo step through the same
//!   `MoveNodesCmd` the drag / nudge journal — the AI-first peer of an align
//!   toolbar, no GUI chrome needed (`query node.<id>.{x,y}` reads the result).
//!   Align needs ≥2 selected, distribute ≥3; the move loop is the shared
//!   `apply_node_moves` SSOT (nudge / align / distribute).
//! - **Auto-layout** (R1383): `invoke auto_layout` (no args) tidies the WHOLE
//!   graph into a layered left-to-right (Sugiyama) arrangement — data flows
//!   forward across columns, vertical order crossing-reduced — in ONE undo step
//!   through the same `apply_node_moves` SSOT. The AI-first peer of an editor's
//!   "arrange" command; the pure geometry is `pinion_node_graph::Layered`
//!   (cycle-broken,
//!   longest-path layered, barycenter-ordered, deterministic — it reads only the
//!   node set + heights + edges, never the current positions).
//! - **Force layout** (R1390): `invoke force_layout` (no args) relaxes the WHOLE
//!   graph into a force-directed (organic) cluster — every node repels every
//!   other, every edge springs its endpoints together, annealed to a compact
//!   symmetric rest state — in ONE undo step through the same `apply_node_moves`
//!   SSOT. The organic counterpart to `auto_layout`'s layered tidy (the yEd
//!   "organic" / Graphviz `neato` mode a pro editor offers beside "arrange");
//!   the pure geometry is `pinion_node_graph::Organic` (grid-seeded, fixed-iteration,
//!   deterministic — it too reads only the node set + edges, never positions).

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pinion_a11y::{
    AccessNode, AccessState, AriaRole, MenuItemCell, ToolbarControl, WidgetA11y, menu_item_nodes,
    toolbar_button_nodes,
};
use pinion_core::animation::Tickable;
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::composite_tag::{
    parse_pair as parse_typed_pair, split_send_payload, split_subindex,
};
use pinion_core::event::LINE_HEIGHT_PX;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, DragPayload, DropPoint, External,
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    RepaintOwner, SchemaArg, SchemaField, ThreadOwnership, int_of,
};
use pinion_core::reactive::{Owner, Signal, batch};
use pinion_core::region::{Point, Region, RegionFit};
use pinion_core::scene::{
    ContainerNode, PathCommand, PathNode, PathPoint, Rect, ScrollAxis, ScrollNode, TextNode,
};
use pinion_core::storage::Storage;
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, PathStyle, Size,
    Stroke, StrokeCap, TextStyle, quantize_unit_byte,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::undo::{
    UndoCommand, UndoStack, UndoStackExternal, undo_redo_verb, use_undo_stack,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::{TextFieldState, blur_committing_field_extra};
use pinion_core::{Color, Command, DragLatch, Frame, Modifiers, Scene, SelectionChord, WidgetCore};
use pinion_graph::Sugiyama;
use pinion_node_graph::{
    Conversion, Document, Extent, Layered, Link as Edge, LinkId as EdgeId, Node, NodeBody, NodeId,
    NodeKind, Organic, Port, PortRef, ROOT, Rewired, Side, Signature, Socket, Tree, TreeId,
};
use pinion_platform_storage::{AppStorage, use_app_storage};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::text_field as tf_paint;
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
/// `add_node` RPC verb) can create, as `(title, input_types, output_types,
/// op)`. A tiny material-graph vocabulary (sources / ops / sink), the same typed
/// port shapes [`default_graph`] seeds. R898 — the entries are
/// [`PortType`]-typed; the first five keep their pre-R898 indices (0..=4) so
/// the index-addressed `add_node(kind)` callers are unmoved, and the typed
/// sources/ops (`Scalar`, `Lerp`) that exercise the type lattice are
/// *appended* (5, 6). R1255 — a fourth element pins each kind's [`MaterialOp`]
/// (the compute identity the evaluator keys off), so kind → op lives in this
/// one SSOT rather than a parallel `match` that could drift from the ordering.
/// R1596 — the ports are gone from this table because a kind declares its own
/// ([`NodeKind::inputs`] / [`NodeKind::outputs`]). They were here when a node
/// STORED its port list, and `node_invariants_hold` existed largely to check the
/// stored list still matched this row.
const PALETTE: &[(&str, MaterialOp)] = &[
    ("Texture", MaterialOp::Texture),
    ("Color", MaterialOp::Color),
    ("Multiply", MaterialOp::Multiply),
    ("Add", MaterialOp::Add),
    ("Output", MaterialOp::Output),
    // R898 — a scalar source: its `Float` output broadcasts into a `Vector`
    // input (accepted) but a `Vector` never narrows into it (rejected).
    ("Scalar", MaterialOp::Scalar),
    // R898 — a 3-input op whose last input is a `Float` factor, so a
    // `Color`/`Texture` (`Vector`) wired to it is type-rejected.
    ("Lerp", MaterialOp::Lerp),
];

/// R849 — sidebar width for the node palette. The canvas keeps its
/// `WIN_W × WIN_H` coordinate system; the palette is an extra left strip, so
/// the capture-drag / hit-test canvas-extent math is unchanged (the
/// `capture_normalize` reference is the offset `GRAPH_TAG` rect).
const PALETTE_W: u32 = 132;
/// R849 — the palette container tag (a11y `toolbar` root; routes no pointer
/// events itself — only its `node_graph#palette_<idx>` item cards do).
const PALETTE_TAG: &str = "node_palette";
/// R1411 §5.51 — the [`DragPayload::kind`] a palette card carries when it is
/// DRAGGED onto the canvas to instantiate its node at the drop point, as
/// opposed to a press-release IN PLACE (which adds the node at the spawn point).
/// Distinct from the `"node-edge"` wire-drag kind so [`drag_release`] routes the
/// two gestures apart, and it is the discriminator the generic drop substrate
/// matches on ([`DragPayload`] doc).
///
/// [`drag_release`]: NodeGraphExternal::drag_release
const PALETTE_DRAG_KIND: &str = "palette-node";
/// R1220 — the pin-drop create menu width (canvas px).
const PIN_CREATE_MENU_W: u32 = 150;
/// R1223 — the create-menu search-header separator glyph (▸ U+25B8), named per
/// the repo's non-ASCII-literal convention rather than inlined at the two use
/// sites.
const PIN_MENU_ARROW: &str = "\u{25b8}";
/// R1220 — the create menu container's composite sub (`node_graph#pin_menu`):
/// a press on the menu's own chrome (header / padding) carries this sub, so the
/// click-away dismiss guard leaves the menu open (only a press *outside* it, or
/// on a `create_<idx>` card, is actioned).
const PIN_MENU_SUB: &str = "pin_menu";
/// R849 — the editor a11y root wrapping the palette + the canvas.
const ROOT_TAG: &str = "node_editor";
/// R916 — the Details panel (right sidebar) width + container tag. The panel
/// reflects the single selected node's properties as rows; it is a sibling of
/// the canvas (outside `GRAPH_TAG`), so the capture-drag / hit-test math is
/// unchanged — like the palette, it merely sits beside the canvas.
const DETAIL_W: u32 = 192;
const DETAIL_TAG: &str = "node_details";
/// R916 — the full editor width: palette + canvas + Details panel. The root
/// container's declared size AND the window's `SizeStrategy` width MUST be this
/// same value — if the window is narrower the flex row shrinks the sidebars and
/// the canvas shifts off its `PALETTE_W` offset (the node / wire geometry every
/// demo's coordinates assume). One const so the two cannot drift.
const TOTAL_W: u32 = PALETTE_W + WIN_W + DETAIL_W;

/// R851 — the [`UndoStackExternal`] anchor: the AI-first undo-history surface
/// (`query can_undo` / `index` / `undo_label`; `invoke undo` / `redo` / `clear`),
/// reached at `/node_undo/external/<slot>`. No `#` in the tag, so it routes as
/// its own coordinator (never confused with a `node_graph#…` composite).
const UNDO_TAG: &str = "node_undo";
/// R851 — the [`use_undo_stack`] cache key: the reducer-side recorder (the
/// coordinator), the keyboard `Ctrl+Z` path, and the [`UndoStackExternal`] all
/// resolve the same shared [`UndoStack`] from this key.
const UNDO_KEY: &str = "node_graph.undo";

/// R878 / R901 — the shared inline edit field (a `TextFieldExternal` extra,
/// the R790 todomvc `EDIT_TF` modal-member shape): ONE field painted over the
/// target's title (a rename) or a pin's default label (a port-default edit)
/// while an edit is in flight. No `#` in the tag — it routes as its own
/// coordinator, like [`UNDO_TAG`].
const EDIT_TF_TAG: &str = "node_edit";
/// R878 — commit-on-blur intent the inline field raises on a click-away
/// (R793 opt-in `with_blur_intent`).
const EDIT_TF_BLUR_INTENT_TAG: &str = pinion_core::intent_tag!("node_edit", "blur");

/// R852 — the per-OS data dir name for the file-backed graph store
/// (`open_app_storage`); the `Owner::cache` key for the shared storage hook.
const STORAGE_APP_NAME: &str = "pinion-node-editor";
const STORAGE_CACHE_KEY: &str = "node_graph.storage";
/// R852 — the single [`Storage`] key the whole graph snapshot is written under
/// (one blob, so `FileStorage`'s tempfile + rename covers the whole save).
const STORAGE_KEY: &str = "node_graph.state";
/// R852 — bump on an incompatible [`SerializedGraph`] layout change; a load of a
/// mismatched version starts fresh (silent fall-through, the todomvc precedent).
// R898 -> 2: typed ports changed the `GraphNode` serialised shape
// (`inputs`/`outputs` counts -> `input_ports`/`output_ports` typed lists).
// R899 -> 3: added per-port `input_defaults` (typed `CellValue`s). R1227 -> 4:
// comment `frames`. R1242 -> 5: the `is_reroute` discriminator (R1259 dropped
// the stored field — `is_reroute` is now DERIVED from `op`; an old blob's
// now-unknown key is ignored on load). R1255 -> 6: the first-class `op` compute
// identity. R1257 -> 7: the source `output_const`. Each bump mismatch-rejects a
// stale blob (`schema_version` gate) so it starts fresh rather than misreading.
// NOTE (R1259): the per-field serde-evolution styles differ and are NOT a
// uniform "no default" doctrine — `op` (bare enum) hard-fails deserialize when
// absent, `frames` carries `#[serde(default)]`, and `output_const` (an `Option`)
// is implicitly default-`None`. The uniform
// gate is the `schema_version` check + R1258 structural validation, not any
// single field's absence.
// R1596 -> 8: the blob is a `Document` now. Six parallel fields (nodes, edges,
// frames and three id counters) collapse into one, so EVERY key changed; a v7
// blob would fail to deserialize anyway, and the bump is what makes that a
// stated rejection ("this file is older than this editor") rather than an
// incidental one that reads like a corrupt file.
// R1599 -> 9: `Port`'s type and resting value moved inside `Flow`, because a
// CONTROL port has neither. The tree's `interface` stores ports, so every
// document holding a group definition changed shape. Enforced now rather than
// remembered: `PERSISTED_SHAPE_HISTORY` below is checked by
// `pinion_core::test_fixtures::assert_persisted_shape`, so a shape change
// cannot reach a commit without a new version.
const PERSISTED_SCHEMA_VERSION: u32 = 9;

/// R1599 — the append-only `(version, digest)` ledger the persistence gate
/// reads. See `pinion_core::test_fixtures::assert_persisted_shape` for why it
/// is append-only and why both columns must stay unique.
///
/// The ledger **opens at 9**, which is this round. Versions 1-8 predate the
/// gate and no digest of them was ever taken, so there is none to record —
/// writing one now would be inventing a measurement rather than reporting one.
#[cfg(test)]
const PERSISTED_SHAPE_HISTORY: &[(u32, u64)] = &[(9, 0xcf24_4e33_beee_b4c5)];

/// R849 — where a newly added node first lands, and the per-add cascade step
/// (in minted-id order) so repeated adds do not stack exactly.
const SPAWN_X: i32 = 300;
const SPAWN_Y: i32 = 44;
const SPAWN_STEP: i32 = 26;
/// R1220 — the gap (graph units) placing an `open_pin_create` RPC-spawned node
/// to the right of its source (the live gesture uses the drop point instead).
const PIN_CREATE_GAP: i32 = 40;

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

/// R1243 — a reroute knot's diameter (logical px). A reroute node
/// ([`NodeGeometry::is_reroute`]) is a wire-routing passthrough, not a compute op,
/// so it paints as this compact circular dot instead of a full card: no header,
/// port rows, or inline editors. Its width == height == this, and both its ports
/// anchor at its centre ([`knot_center`]) — the wire passes straight through the
/// dot (the Blueprint / material-editor reroute look).
const KNOT_SIZE: i32 = 18;

/// R1227 — comment-frame geometry. `FRAME_PAD` is the margin an
/// `add_frame`-around-selection leaves on every side of the selected nodes'
/// bounding box; `FRAME_HEADER_H` is the title strip height; `FRAME_FILL_ALPHA`
/// the translucent body wash (the frame reads as a wash BEHIND the nodes, never
/// obscuring them).
const FRAME_PAD: i32 = 28;
const FRAME_HEADER_H: i32 = 24;
/// R1234 — the smallest a comment frame can be resized to (its chrome height:
/// the title strip + one pad), so a `resize` can never collapse the box below
/// its own header. Both axes share it — a frame narrower than its title is as
/// useless as one shorter than its header.
const FRAME_MIN: i32 = FRAME_HEADER_H + FRAME_PAD;
const FRAME_FILL_ALPHA: u8 = 28;
/// The comment-frame accent (a muted amber, distinct from the port-type
/// colours) — the fill is this at [`FRAME_FILL_ALPHA`], the border + title opaque.
const FRAME_COLOR: Color = Color::rgb(0xc8, 0x9b, 0x3c);

/// R880 — the marquee rubber band's fill alpha (a translucent wash of the
/// accent; the border stays fully opaque).
const MARQUEE_FILL_ALPHA: u8 = 40;

// ─── R877 viewport (pan = ScrollState, zoom = shared Signal) ───────

/// R877 — the world extent (graph units, both axes): the finite huge
/// canvas every desktop node editor uses (the engine's blueprint graph is
/// likewise bounded, just vast). Pan = a [`ScrollAxis::Both`] scroll
/// over this world; the scene substrate is unsigned so the world's
/// origin is its top-left corner and node coordinates stay `>= 0`.
const WORLD: i32 = 2048;
/// R877 — zoom bounds. The floor keeps `WORLD × zoom` no smaller than
/// the canvas viewport (so the scroll maxima never collapse below 0
/// and the world always fills the view); the ceiling is the usual
/// 400% detail zoom.
const ZOOM_MIN: f64 = 0.5;
const ZOOM_MAX: f64 = 4.0;
/// R877 — zoom factor per wheel notch (`Ctrl`+wheel) and per keyboard
/// step (`Ctrl`+`=` / `Ctrl`+`-`). `dy / LINE_HEIGHT_PX` recovers the
/// notch count for the zoom exponent ([`External::wheel`] hands `Lines`
/// deltas pre-scaled by that same contract constant).
const ZOOM_STEP: f64 = 1.2;
/// R877 — margin (screen px) `frame_all` keeps around the node bbox.
const FRAME_MARGIN: i32 = 24;

/// R1182 — edge-drag auto-pan hot zone: the fraction of the canvas, measured
/// in from each edge, within which a live node drag auto-scrolls the viewport
/// toward that edge (the DCC / the engine convention — a node dragged to the
/// rim keeps going without releasing). 0.12 of the 640×420 canvas ≈ a 77 px /
/// 50 px rim.
const AUTOPAN_MARGIN: f64 = 0.12;
/// R1182 — auto-pan speed at full rim penetration, in world px per second (the
/// [`ScrollState`] offset basis). Scaled by the frame `dt` and the linear
/// penetration depth, so the pan eases in from the margin line (0) to full
/// speed at the very rim (1).
const AUTOPAN_SPEED: f64 = 900.0;

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

/// `u32` → `i32`, [`upx`]'s inverse for arithmetic back in signed graph space
/// (R1358 rebases a wire's commands by its `Rect`'s already-clamped origin).
fn ipx(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
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

// ─── graph model (the taxonomy; the mechanism is `pinion-node-graph`) ──

// R1596 — the model itself is the crate's. Before this round the editor carried
// its own `Vec<Node<MaterialOp>>` + `Vec<Edge>` beside `pinion-node-graph`, so the tree
// held TWO node models with two sets of invariants, two evaluators and two
// notions of what a frame is; R1577 created that duplication by lifting the
// mechanism out of here, and R1593 (a directed type relation), R1594 (a value
// authored per node) and R1595 (a frame's height) were the three things the
// crate could not express that kept it open. What stays here is what this crate
// deliberately does not have: the taxonomy ([`MaterialOp`]), the socket type
// lattice ([`PortType`]) and the card geometry ([`NodeGeometry`]).

// R1596 — the two stable handles are the crate's, imported under this editor's
// own vocabulary: a wire is a `Link` there and an "edge" here, and one import
// makes the two names one type rather than two that must be converted.
//
// The editor's own `NodeId` / `EdgeId` were the same shape (a `u32` newtype,
// minted once, never reused) for the same reason the crate's are: an edge, a
// selection or an undo record that names a node has to survive the deletion of
// *other* nodes.

/// R1227 / R1596 — a comment frame is a **node** (`NodeBody::Frame`), so its
/// handle is a [`NodeId`] and it is minted from the same counter.
///
/// That is the DCC's model (`NODE_FRAME` is an ordinary node type) and it is what makes
/// containment a fact the document maintains — `Node::parent`, whose forest invariants
/// the crate enforces — rather than a rectangle the paint re-derives
/// membership from every frame.
type FrameId = NodeId;

/// R898 — a port's data type. The lattice that makes the graph a *typed* node
/// editor: an edge connects an output to an input only when a value can cross
/// from the output's type to the input's ([`MaterialOp::conversion`]), so the canvas rejects an
/// ill-typed wire the way the engine's blueprint / material graphs do. Two
/// material-graph types (`Float` scalar, `Vector` colour/vec3).
///
/// R1596 — the relation is **directed** and is now declared as the conversion
/// itself, once, in [`NodeKind::conversion`]: `is_assignable_to` was this
/// example's own half of that answer and the crate held the other, which is the
/// asymmetry R1593 made expressible at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
enum PortType {
    /// A scalar.
    Float,
    /// A vec3 / colour (a material graph's primary data type).
    Vector,
}

/// R898 — a `Float` port's signature colour. Node editors colour-code pins by type
/// (the engine/the DCC) so a connection's validity is legible at a glance; the
/// colour is a fixed type identity, *not* themed (a `Float` reads the same green
/// in light and dark).
const FLOAT_PORT_COLOR: Color = Color::rgb(0x7c, 0xd0, 0x6f);
/// R898 — a `Vector` port's signature colour (gold, the vec3 convention).
const VECTOR_PORT_COLOR: Color = Color::rgb(0xe0, 0xb0, 0x3a);

impl PortType {
    /// The pin's signature colour (the `view_port` fill). Fixed per type — the
    /// node-editor colour-coding convention, not a themed role.
    const fn color(self) -> Color {
        match self {
            PortType::Float => FLOAT_PORT_COLOR,
            PortType::Vector => VECTOR_PORT_COLOR,
        }
    }

    /// R1599 — the ink for a port that carries no value. Blueprint draws its
    /// execution pins as plain white arrows for the same reason: the pin's
    /// colour is the type's, and control has none.
    const CONTROL_INK: Color = Color::rgb(0xF2, 0xF2, 0xF2);

    /// The wire-form / display token — the `input_types` CSV element and the
    /// `(<n>/<m>)` palette label has no use for it, but the AI-first
    /// `query node.<id>.input_types` does.
    const fn name(self) -> &'static str {
        match self {
            PortType::Float => "Float",
            PortType::Vector => "Vector",
        }
    }

    /// R899 — the literal default for an *unconnected* input port of this type
    /// (the blueprint / material-graph "pin default value"): a typed
    /// [`CellValue`] the data-grid value substrate carries, so a port default
    /// reuses its parse / display / typed-`intervene` machinery rather than a
    /// bespoke per-type literal. `Float` -> a scalar `0.0`, `Vector` -> a
    /// mid-grey colour (a vec3/colour seed an author overrides).
    fn default_value(self) -> CellValue {
        match self {
            PortType::Float => CellValue::Float(0.0),
            PortType::Vector => CellValue::Color(Color::rgb(0x80, 0x80, 0x80)),
        }
    }

    /// Whether an output of `self` may feed an input of `into`. Exact match
    /// always; a scalar `Float` broadcasts up to a `Vector` (the shader-graph
    /// scalar-promote coercion). There is no narrowing (a `Vector` never feeds
    /// a `Float`), so the relation is a strict partial order, not symmetric.
    ///
    /// R1596 — **derived** from [`MaterialOp::conversion`] rather than declared
    /// beside it. Whether a wire is legal and what arrives along it were two
    /// statements here (this predicate and [`NodeKind::conversion`]) that could disagree;
    /// R1593 made the crossing one declaration, so this is now its yes-or-no
    /// reading and the two cannot.
    fn is_assignable_to(self, into: PortType) -> bool {
        MaterialOp::conversion(&self, &into).is_allowed()
    }

    /// R1596 — a port of this type, named and carrying its own resting value.
    ///
    /// The kind says what a port *is*; [`Node::values`] says what one node's
    /// port has been *given* (R1594). Both defaults the editor used to store per
    /// node — `input_defaults` and `output_const` — are that one mechanism, so
    /// the seed value lives here and only an authored override lives on a node.
    fn port(self, name: &str) -> Port<PortType, CellValue> {
        Port::new(name, self).with_default(self.default_value())
    }

    /// R1596 — a port of this type whose RESTING value is the type's default.
    ///
    /// The same construction as [`port`](Self::port), named apart because on an
    /// **output** a declared default means "what this node yields when the kind
    /// computed nothing", which is true of a source and false of everything
    /// else ([`NodeKind::outputs`]).
    fn resting(self, name: &str) -> Port<PortType, CellValue> {
        self.port(name)
    }
}

/// R1255 §5.38 §5.52 / R1596 — a node's **compute operation**, and this
/// editor's whole half of the node system: the taxonomy.
///
/// `pinion-node-graph` supplies the model, the invariants, the groups, the
/// frames and the evaluator, and asks the application for exactly this — what a
/// node *is*, what ports it has, what it computes, and how its socket types
/// relate. The R1259 audit recorded the defect this closes: the op taxonomy and
/// the evaluation mechanism were FUSED in a closed `match` in this module, and
/// the doc said the split "is what a future trait/registry-dispatched
/// `pinion-node-graph` crate would provide". It exists; this is the split.
///
/// Identity for evaluation is this op and never the freely-rewritable label (the
/// R1242 lesson — a node renamed "Foo" still multiplies), which is the rule
/// [`NodeKind::name`] states for every taxonomy rather than one this example
/// keeps for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
enum MaterialOp {
    /// Source — a texture sample. Its output is the value authored on its own
    /// output port (R1594), not a computed one.
    Texture,
    /// Source — a constant colour (`Vector`).
    Color,
    /// `Vector × Vector → Vector`: component-wise multiply blend (`a·b/255`).
    Multiply,
    /// `Vector + Vector → Vector`: component-wise add (saturating at 255).
    Add,
    /// Sink — no output; its resolved input 0 is the graph's terminal value.
    Output,
    /// Source — a constant scalar (`Float`).
    Scalar,
    /// `lerp(Vector, Vector, Float) → Vector`: component-wise, factor clamped
    /// to `0..=1`.
    Lerp,
    /// R1242 — a wire-routing passthrough carrying the type it routes, so a
    /// knot spliced into a `Float` wire is a `Float` knot.
    ///
    /// R1596 — the type is a **payload** because a kind declares its own ports:
    /// the pre-R1596 model stored a port list per node and a bare `Reroute`
    /// beside it, and `node_invariants_hold` existed largely to check the two
    /// agreed. A kind that carries its wire type cannot disagree with itself.
    Reroute(PortType),
}

impl NodeKind for MaterialOp {
    type Type = PortType;
    type Value = CellValue;

    /// R1256 — the op's canonical, rename-stable identity token (the `Add`/
    /// `Multiply`/... a `query node.<id>.op` reports). This is the AI-legible
    /// answer to "what does this node compute" — needed because the structural
    /// reads cannot distinguish ops that share a signature (`Add` and `Multiply`
    /// are both `(Vector,Vector)→Vector`; `Texture` and `Color` both
    /// `()→Vector`) and the label is freely rewritable.
    fn name(&self) -> String {
        match self {
            MaterialOp::Texture => "Texture",
            MaterialOp::Color => "Color",
            MaterialOp::Multiply => "Multiply",
            MaterialOp::Add => "Add",
            MaterialOp::Output => "Output",
            MaterialOp::Scalar => "Scalar",
            MaterialOp::Lerp => "Lerp",
            MaterialOp::Reroute(_) => "Reroute",
        }
        .to_owned()
    }

    fn inputs(&self) -> Vec<Port<PortType, CellValue>> {
        match self {
            MaterialOp::Texture | MaterialOp::Color | MaterialOp::Scalar => Vec::new(),
            MaterialOp::Multiply | MaterialOp::Add => {
                vec![PortType::Vector.port("A"), PortType::Vector.port("B")]
            }
            MaterialOp::Output => vec![PortType::Vector.port("In")],
            MaterialOp::Lerp => vec![
                PortType::Vector.port("A"),
                PortType::Vector.port("B"),
                PortType::Float.port("Factor"),
            ],
            MaterialOp::Reroute(ty) => vec![ty.port("In")],
        }
    }

    /// R1596 — **only a SOURCE's output declares a resting value.**
    ///
    /// R1594's rule covers both sides of a node with one sentence — an authored
    /// value is what a port carries when nothing else supplies one, and for an
    /// output that means the kind computed nothing there — so what a kind
    /// declares on an *output* decides what a node yields when it cannot
    /// compute. For a source that is the whole point: it computes nothing, ever,
    /// and its constant is the resting value.
    ///
    /// For a COMPUTE op it would be a lie. An `Add` whose inputs do not resolve
    /// has no value, and giving its output a resting grey would make a node on a
    /// **cycle** evaluate to a plausible colour instead of to nothing — which is
    /// exactly the `null` R1260's debugger reads use to localise the cycle. The
    /// first draft of this migration declared a default on every port and that
    /// is precisely what happened, caught by `r1255_a_cycle_is_uncomputable`.
    fn outputs(&self) -> Vec<Port<PortType, CellValue>> {
        match self {
            MaterialOp::Texture | MaterialOp::Color => vec![PortType::Vector.resting("Out")],
            MaterialOp::Scalar => vec![PortType::Float.resting("Out")],
            MaterialOp::Multiply | MaterialOp::Add | MaterialOp::Lerp => {
                vec![Port::new("Out", PortType::Vector)]
            }
            MaterialOp::Output => Vec::new(),
            MaterialOp::Reroute(ty) => vec![Port::new("Out", *ty)],
        }
    }

    /// R1255 — compute every output from the already-resolved, port-typed
    /// `inputs` (each converted to the declared [`PortType`] on the way in by
    /// the crate's evaluator, so a `Vector` input is a [`CellValue::Color`] and a
    /// `Float` input a [`CellValue::Float`]). A `None` slot is an *unresolvable*
    /// input (a cycle upstream); a compute op that needs it yields `None`.
    ///
    /// R1596 — a **source computes nothing**, and answering `None` is what makes
    /// its constant the same mechanism every other authored port value uses: the
    /// crate fills an output the kind left empty from [`Node::values`], else from
    /// this kind's own [`Port::default_value`]. Before this round the source arms
    /// returned the type default here AND the node stored an `output_const`
    /// beside it — one fact in two places, which is the shape R1594 named.
    fn evaluate(&self, inputs: &[Option<CellValue>]) -> Vec<Option<CellValue>> {
        // A required input at position `i` (present *and* resolved).
        let req = |i: usize| inputs.get(i).and_then(Option::as_ref);
        // One match answering the whole output vector, so a kind's ARITY and its
        // values are one statement. Splitting them ("compute a value, then
        // decide whether there is a slot for it") made a sink's empty vector and
        // a source's unresolved slot look like the same `None`, and they are
        // opposites: a sink has nowhere to put a value, a source has somewhere
        // and the value is authored rather than computed.
        match self {
            // A source computes nothing — its output is the value authored on
            // its own port (R1594), which the crate supplies where this leaves a
            // hole.
            MaterialOp::Texture | MaterialOp::Color | MaterialOp::Scalar => vec![None],
            MaterialOp::Add => vec![
                req(0)
                    .and_then(as_color)
                    .zip(req(1).and_then(as_color))
                    .map(|(a, b)| CellValue::Color(color_add(a, b))),
            ],
            MaterialOp::Multiply => vec![
                req(0)
                    .and_then(as_color)
                    .zip(req(1).and_then(as_color))
                    .map(|(a, b)| CellValue::Color(color_mul(a, b))),
            ],
            MaterialOp::Lerp => vec![
                req(0)
                    .and_then(as_color)
                    .zip(req(1).and_then(as_color))
                    .zip(req(2).and_then(as_float))
                    .map(|((a, b), t)| CellValue::Color(color_lerp(a, b, t))),
            ],
            MaterialOp::Reroute(_) => vec![req(0).cloned()],
            // A sink has no output at all — not an output carrying no value.
            MaterialOp::Output => Vec::new(),
        }
    }

    /// R1594 — which socket type a value is, so an authored one is checked
    /// against the port it is written to.
    fn value_type(value: &CellValue) -> Option<PortType> {
        match value {
            CellValue::Float(_) => Some(PortType::Float),
            CellValue::Color(_) => Some(PortType::Vector),
            // The value substrate carries kinds this lattice has no port for
            // (text, int, bool); saying so is not the same as guessing.
            _ => None,
        }
    }

    /// R1593 / R1596 — whether and how a value crosses from one socket type to
    /// another. Exact match passes through unchanged; a scalar `Float`
    /// broadcasts up to a `Vector` (the shader-graph scalar-promote coercion);
    /// a `Vector` never narrows back.
    ///
    /// **The relation is directed, and that is why it is one declaration.** This
    /// example is what R1593 was built for: `is_assignable_to` said whether a
    /// wire was legal and [`NodeKind::conversion`] said what arrived along it, two
    /// statements over the same asymmetric lattice with nothing tying them
    /// together — and no *equality* relation can hold an asymmetry, which is why
    /// the crate could not host this model at all before that round.
    fn conversion(from: &PortType, to: &PortType) -> Conversion<CellValue> {
        match (from, to) {
            (PortType::Float, PortType::Float) | (PortType::Vector, PortType::Vector) => {
                Conversion::Direct
            }
            (PortType::Float, PortType::Vector) => Conversion::Converted(|value| match value {
                CellValue::Float(f) => Some(CellValue::Color(broadcast_scalar(f))),
                // A `Float` port holding a non-float is a document that came
                // from elsewhere; it does not cross rather than crossing wrong.
                _ => None,
            }),
            (PortType::Vector, PortType::Float) => Conversion::Refused,
        }
    }
}

/// R1255 — the `Color` (`Vector`) payload of `value`, or `None` if it is not a
/// colour (a defensive guard — [`resolve_input_value`] coerces to the port type, so a
/// well-typed graph always matches).
fn as_color(value: &CellValue) -> Option<Color> {
    match value {
        CellValue::Color(c) => Some(*c),
        _ => None,
    }
}

/// R1255 — the `f64` (`Float`) payload of `value`, or `None` if it is not a
/// float (the `Float`-input twin of [`as_color`]).
fn as_float(value: &CellValue) -> Option<f64> {
    match value {
        CellValue::Float(f) => Some(*f),
        _ => None,
    }
}

// R1255 — the material-graph colour ops (`Add` / `Multiply` / `Lerp`) work in
// **raw sRGB-byte component space** (arithmetic straight on the `0..=255`
// channels), the transparent shader-authoring convention for this illustrative
// substrate — NOT [`pinion_core::style::Color::lerp`]'s perceptual
// **linear-space** interpolation (`to_linear → mix → from_linear`), which is the
// right model for UI animation but would make the eval values non-round and the
// mechanism harder to read/pin (`grey·grey/255 = 64` is a clean unit oracle).
// Photometric linear-space colour math is a Phase-C *renderer* concern, not this
// authoring-graph illustration ([[use-substrate-audit-contract-vs-glue]] — the
// substrate lerp is a *different operation*, not the same one hand-rolled).

/// R1255 — component-wise saturating add (the `Add` op). Alpha stays opaque.
fn color_add(a: Color, b: Color) -> Color {
    Color::rgb(
        a.r.saturating_add(b.r),
        a.g.saturating_add(b.g),
        a.b.saturating_add(b.b),
    )
}

/// R1255 — component-wise multiply blend (the `Multiply` op): `a·b/255` per
/// channel, the standard 0..255 multiply. `a·b ≤ 255·255` fits `u16`, and the
/// `/255` result is `≤ 255`, so the byte conversion never saturates in practice.
fn color_mul(a: Color, b: Color) -> Color {
    let ch = |x: u8, y: u8| u8::try_from(u16::from(x) * u16::from(y) / 255).unwrap_or(255);
    Color::rgb(ch(a.r, b.r), ch(a.g, b.g), ch(a.b, b.b))
}

/// R1255 — component-wise linear interpolation (the `Lerp` op): `a + (b−a)·t`
/// per channel, `t` clamped to `0..=1` (a factor outside the unit interval is
/// not an extrapolation here). Alpha stays opaque.
fn color_lerp(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| quantize_unit_byte(f64::from(x) + (f64::from(y) - f64::from(x)) * t);
    Color::rgb(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

/// R1255 — broadcast a scalar `Float` to a `Vector` (`Color`): the shader-graph
/// scalar-promote coercion ([`PortType::is_assignable_to`] permits `Float →
/// Vector`). The scalar is treated as a normalized `0..=1` channel — `f·255`
/// per channel — so `0.0 →` black, `1.0 →` white, `0.5 →` mid-grey.
fn broadcast_scalar(f: f64) -> Color {
    let c = quantize_unit_byte(f.clamp(0.0, 1.0) * 255.0);
    Color::rgb(c, c, c)
}

/// R1596 — the material graph as a document.
///
/// One `Document` replaces the five reactive cells the editor used to keep
/// (`use_nodes` / `use_edges` / `use_frames` and two id counters): the crate
/// mints ids, holds the frames as nodes, and maintains the invariants those
/// separate cells could only be checked against after the fact.
type Graph = Document<MaterialOp>;

/// The one tree this editor edits.
///
/// Groups are a whole second axis of a node editor and this example is the
/// *canvas* one — `hello-node-groups` is the binding that composes the crate's
/// group operations. Naming the tree once means the day this editor grows a
/// breadcrumb, the change is this constant becoming a signal.
const TREE: TreeId = ROOT;

/// R1255 — the value that resolves into `node`'s input `port`.
///
/// R1596 — one call into the crate's evaluator, which resolves a wired source
/// (converting it by [`MaterialOp::conversion`]), else the value authored on
/// this node's port, else the kind's own resting value. The editor used to hold
/// all three branches plus its own memo and its own on-stack cycle guard.
fn resolve_input_value(graph: &Graph, node: NodeId, port: usize) -> Option<CellValue> {
    graph
        .evaluator()
        .input(TREE, Socket::new(node, uport(port)))
}

/// R1255 / R1596 — the value node `id` presents.
///
/// A node with outputs presents its first one. A **sink** has none, and what it
/// presents is what flows INTO it — the reading `node.<id>.value` has always
/// had for an `Output` node, kept here as the app-level derivation it is: the
/// crate answers "every output" and a sink honestly has zero.
fn evaluate(graph: &Graph, id: NodeId) -> Option<CellValue> {
    let mut evaluator = graph.evaluator();
    let outputs = evaluator.outputs(TREE, id);
    if outputs.is_empty() {
        return evaluator.inputs(TREE, id).into_iter().next().flatten();
    }
    outputs.into_iter().next().flatten()
}

/// R1260 — an evaluated `CellValue` as its introspection wire form, or
/// [`IntrospectValue::Null`] when it is absent (a cycle upstream / no value).
/// The one home for the `value` / `resolved_input.<port>` / `eval.output` reads'
/// shared "computed value else null" mapping (a 3rd consumer crossed the lift
/// threshold at R1260).
fn cell_or_null(value: Option<CellValue>) -> IntrospectValue {
    value.map_or(IntrospectValue::Null, |v| v.to_introspect())
}

/// R1255 — the graph's terminal value: the resolved input of the
/// [`MaterialOp::Output`] sink (lowest id when several exist, so the read is
/// deterministic). `None` when there is no `Output` node, or its input cone has
/// a cycle. Behind `query eval.output`.
fn eval_terminal(graph: &Graph) -> Option<CellValue> {
    let sink = kind_nodes(graph)
        .filter(|(_, op)| *op == MaterialOp::Output)
        .map(|(node, _)| node.id)
        .min()?;
    evaluate(graph, sink)
}

/// R1596 — every node of the taxonomy, with its op, ascending by id.
///
/// A frame is a node too ([`NodeBody::Frame`]), and every read named `node.*`
/// on this surface means a node that *computes*. One derivation, so no read has
/// to remember to filter and none of them can disagree about what a node is.
fn kind_nodes(graph: &Graph) -> impl Iterator<Item = (&Node<MaterialOp>, MaterialOp)> {
    graph
        .tree(TREE)
        .into_iter()
        .flat_map(Tree::nodes)
        .filter_map(|node| match &node.body {
            NodeBody::Kind(op) => Some((node, *op)),
            _ => None,
        })
}

/// R1596 — every node of the taxonomy by value, for the readers that need a
/// slice rather than a stream (the layout metrics, which index by position).
fn kind_node_vec(graph: &Graph) -> Vec<Node<MaterialOp>> {
    kind_nodes(graph).map(|(node, _)| node.clone()).collect()
}

/// R1596 — the node `id`, when it is one of the taxonomy's.
fn kind_node(graph: &Graph, id: NodeId) -> Option<(&Node<MaterialOp>, MaterialOp)> {
    let node = graph.tree(TREE)?.node(id)?;
    match &node.body {
        NodeBody::Kind(op) => Some((node, *op)),
        _ => None,
    }
}

/// R1596 — every frame, ascending by id ([`NodeBody::Frame`]).
fn frame_nodes(graph: &Graph) -> impl Iterator<Item = &Node<MaterialOp>> {
    graph
        .tree(TREE)
        .into_iter()
        .flat_map(Tree::nodes)
        .filter(|node| node.is_frame())
}

/// R1596 — the frame `id`, if `id` names one.
fn frame_node(graph: &Graph, id: FrameId) -> Option<&Node<MaterialOp>> {
    graph.tree(TREE)?.node(id).filter(|node| node.is_frame())
}

/// R1227 / R1596 — the nodes frame `id` holds, as a **fact** rather than a
/// rectangle test ([`Document::members`], one level).
///
/// The editor asked its geometry — "whose centre is inside this box" — on
/// every read, so a frame silently adopted a node dragged over it and
/// abandoned one dragged out, and a resize changed what the frame *said* it
/// held with nobody having edited membership. R1589 made containment a stored
/// relation the document maintains, which is the DCC's `bNode::parent` and the reason `NODE_OT_join`
/// exists there as an explicit act.
fn frame_members(graph: &Graph, id: FrameId) -> Vec<Node<MaterialOp>> {
    let Some(tree) = graph.tree(TREE) else {
        return Vec::new();
    };
    graph
        .members(TREE, id)
        .into_iter()
        .filter_map(|member| tree.node(member).cloned())
        .collect()
}

/// R879 — one node's move within a gesture: `(id, before, after)` positions.
///
/// A drag / nudge writes positions live and journals the run afterwards, so the
/// *before* half is what the undo step has to reconstruct and this triple is how
/// a caller carries it out of the gesture that observed it.
type NodeMove = (NodeId, (i32, i32), (i32, i32));

/// R1596 — every wire, in insertion order.
fn edges(graph: &Graph) -> &[Edge] {
    graph.tree(TREE).map_or(&[], Tree::links)
}

/// R1596 — the wire `id`.
fn edge(graph: &Graph, id: EdgeId) -> Option<&Edge> {
    graph.tree(TREE)?.link(id)
}

/// R1596 — the signature of node `id`: the ports it presents right now.
fn signature_of(graph: &Graph, id: NodeId) -> Option<Signature<MaterialOp>> {
    graph.signature(TREE, id)
}

/// R1599 — what a port with no socket type is called in a readout. This
/// example's taxonomy declares none, and the constant exists because the model
/// admits one, so a control port would read as itself rather than as `?`.
const CONTROL_PORT: &str = "control";

/// R1596 — the declared type of one of node `id`'s input ports.
fn input_type(graph: &Graph, id: NodeId, port: usize) -> Option<PortType> {
    signature_of(graph, id)?
        .inputs
        .get(port)
        .and_then(|p| p.value_type().copied())
}

/// R1596 — the declared type of one of node `id`'s output ports.
fn output_type(graph: &Graph, id: NodeId, port: usize) -> Option<PortType> {
    signature_of(graph, id)?
        .outputs
        .get(port)
        .and_then(|p| p.value_type().copied())
}

/// R899 / R1596 — the literal an unconnected input port carries: what has been
/// authored on THIS node's port, else the kind's own resting value.
///
/// The editor stored a full `input_defaults` vector on every node, seeded from
/// the port types; the crate splits that into the kind's declaration and the
/// node's override, so a node only carries what an author actually changed.
fn input_default(graph: &Graph, id: NodeId, port: usize) -> Option<CellValue> {
    let declared = signature_of(graph, id)?
        .inputs
        .get(port)?
        .default_value()
        .cloned();
    graph
        .port_value(TREE, id, PortRef::input(uport(port)))
        .cloned()
        .or(declared)
}

/// R1257 / R1596 — the constant a **source** node's output emits: the value
/// authored on output port 0, else the kind's resting value.
fn source_const(graph: &Graph, id: NodeId) -> Option<CellValue> {
    if !is_source(graph, id) {
        return None;
    }
    let declared = signature_of(graph, id)?
        .outputs
        .first()?
        .default_value()
        .cloned();
    graph
        .port_value(TREE, id, PortRef::output(0))
        .cloned()
        .or(declared)
}

/// R1257 / R1596 — whether node `id` is an authorable **source**: it computes
/// nothing from anything, so what its output carries is a value someone typed.
///
/// **Derived from the signature**, where the editor stored an `output_const:
/// Option<CellValue>` whose `Some`-ness was the discriminator and whose
/// consistency with the port list `node_invariants_hold` had to check.
fn is_source(graph: &Graph, id: NodeId) -> bool {
    signature_of(graph, id).is_some_and(|s| s.inputs.is_empty() && !s.outputs.is_empty())
}

/// A port index as the crate spells one.
fn uport(port: usize) -> u32 {
    u32::try_from(port).unwrap_or(u32::MAX)
}

/// A port index as this editor spells one (the inverse of [`uport`]).
fn uidx(port: u32) -> usize {
    usize::try_from(port).unwrap_or(usize::MAX)
}

/// R1383 / R1597 — the arrangement passes the `auto_layout` and `force_layout`
/// verbs run, tuned for this editor's cards.
///
/// R1597 lifted the passes themselves into `pinion-node-graph`
/// ([`Layered`] / [`Organic`]): the projection from a document to the solver —
/// which nodes take part, their extents, their links as index pairs, and where a
/// layer index lands in canvas coordinates — is not application-specific, and it
/// was living here, so every other node-graph application had to copy it. What
/// stays here is what genuinely IS this application's: how wide and tall a card
/// is, and how much air to leave between columns.
///
/// `row_gap` is the vertical clearance between two nodes stacked in one column;
/// `bend_size` the free-axis extent a long edge's BEND occupies in a column it
/// passes through (far smaller than a node — a bend stands for a wire — but
/// never zero, or the compaction could stack two wires on one coordinate); and
/// `sweeps` the barycenter crossing-reduction pass count, fixed so the layout
/// always terminates and is deterministic (ZERO-FLAKE).
const LAYOUT: Layered = Layered {
    sugiyama: Sugiyama {
        row_gap: 24,
        bend_size: 12,
        sweeps: 4,
    },
    // R1383 — horizontal air between two columns. R1597: the COLUMN's own width
    // is no longer part of this number — a column is as wide as its widest card,
    // so a row of reroute knots no longer costs a row of full cards.
    column_gap: 60,
};

/// R1390 / R1597 — the organic pass: a fixed iteration count so the annealing
/// always terminates and the arrangement is reproducible (ZERO-FLAKE), and an
/// ideal spring length of one card plus one gap, so it spaces nodes comparably
/// to the layered pass.
const ORGANIC: Organic = Organic {
    iterations: 200,
    ideal_length: 190.0,
};

/// R1597 — how much canvas one card takes, which is the one thing a layout pass
/// cannot derive for itself: a node's width may be authored
/// (`Appearance::width`) and its height is a function of the ports THIS editor
/// chooses to draw.
fn card_extent(node: &Node<MaterialOp>) -> Extent {
    Extent::new(node.width(), node.height())
}

/// R1441 — the edge-crossing count of a fresh layered layout of this graph: the
/// tidiness metric [`LAYOUT`] is judged by, and the reason the long-edge split is
/// separately observable from the coordinate solver.
///
/// Crossings are a property of the ORDER alone, so this reports what the
/// barycenter sweeps achieved over the PROPER layering — the number the split
/// improves and the coordinate solver provably cannot change. Derived on demand
/// from the current graph rather than cached at the last `auto_layout`, so it
/// cannot go stale after an edit.
fn layout_crossings(graph: &Graph) -> usize {
    LAYOUT
        .run(graph, TREE, (0, 0), card_extent)
        .quality()
        .map_or(0, |q| q.crossings)
}

/// R1441 — `(inner segments, of those drawn straight)` for a fresh layout of this
/// graph: **the paper's guarantee, published as data**.
///
/// Brandes-Köpf promises every inner segment — a run joining two consecutive
/// layers with a bend at each end — comes out vertical. Publishing the pair
/// rather than a ratio means a client can tell "none were straight" from "there
/// were none", which a single number cannot.
fn layout_inner_straightness(graph: &Graph) -> (usize, usize) {
    LAYOUT
        .run(graph, TREE, (0, 0), card_extent)
        .quality()
        .map_or((0, 0), |q| (q.inner_segments, q.straight_inner))
}

/// R1258 / R1596 — whether a document upholds every structural invariant.
///
/// The editor kept three predicates of its own — `node_invariants_hold`,
/// `edges_invariants_hold`, `graph_invariants_hold` — checking unique ids, a
/// node's stored port list against its op's canonical shape, per-port default
/// typing, a source's constant, type-assignable wires and the one-wire-per-input
/// rule. Every one of those is now either impossible to state (a node has no
/// stored port list: the kind declares them) or [`Document::validate`]'s, which
/// also catches four things the editor's never did — a dangling parent, a
/// container that is not a frame, a containment cycle and a dangling group
/// instance.
///
/// Kept as a named predicate because `set_graph` is a *gate*: a blob that
/// arrives over the wire is rejected whole rather than half-loaded.
fn graph_invariants_hold(graph: &Graph) -> bool {
    graph.validate().is_empty()
}

/// R1220 — the first input port index of [`PALETTE`] kind `kind` an output of
/// type `from` may feed (the auto-wire target when a pin-drop creates that
/// node). `None` when the kind has no input assignable from `from` — the exact
/// gate [`pin_create_candidates`] filters on, so a returned candidate always
/// resolves a wire target here.
fn first_compatible_input(kind: usize, from: PortType) -> Option<usize> {
    let &(_, op) = PALETTE.get(kind)?;
    op.inputs()
        .iter()
        .position(|p| p.value_type().is_some_and(|to| from.is_assignable_to(*to)))
}

/// R1220 — the [`PALETTE`] kinds a pin-drop from an output of type `from` may create,
/// in palette order: a kind qualifies iff it has at least one input port
/// assignable from `from` (so the new node can be auto-wired — [`first_compatible_input`] is
/// guaranteed `Some`), and, when `filter` is non-empty, its title contains `filter`
/// (case-insensitive) — the type-to-narrow search the engine / the DCC
/// pin-drop menu offers. A pure fn over `(from, filter)` so both the coordinator (menu
/// candidates + commit gate) and the tests read one SSOT.
fn pin_create_candidates(from: PortType, filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    (0..PALETTE.len())
        .filter(|&k| first_compatible_input(k, from).is_some())
        .filter(|&k| {
            needle.is_empty() || PALETTE[k].0.to_ascii_lowercase().contains(needle.as_str())
        })
        .collect()
}

/// R1596 — the card geometry of a node, which is the application's to know.
///
/// R1589 settled where this belongs: a frame's extent needs card geometry, and a
/// model crate deliberately has none — `Node` carries `x`, `y` and an
/// `Appearance` whose `width` / `height` are *authored* overrides in "the
/// application's own units". So the derivation lives here, as an extension trait
/// on the crate's node, which is what keeps `right()` / `bottom()` one answer
/// across the paint, the marquee, align, distribute and the frame clamp.
///
/// The two arms are the two things a node can be on this canvas: a **frame**,
/// whose size is authored because nothing derives it, and everything else, whose
/// size is a function of how many ports it draws.
trait NodeGeometry {
    /// Width in graph units.
    fn width(&self) -> i32;
    /// Height in graph units.
    fn height(&self) -> i32;
    /// Right extent in graph units.
    fn right(&self) -> i32;
    /// Bottom extent in graph units.
    fn bottom(&self) -> i32;
    /// Whether this node is a wire-routing knot — drawn as a dot, not a card.
    fn is_reroute(&self) -> bool;
    /// The name to paint: the rename if there is one, else the kind's.
    fn title(&self) -> String;
}

impl NodeGeometry for Node<MaterialOp> {
    fn width(&self) -> i32 {
        if self.is_frame() {
            return ipx(self.appearance.width.unwrap_or(upx(FRAME_MIN)));
        }
        if self.is_reroute() { KNOT_SIZE } else { NODE_W }
    }

    fn height(&self) -> i32 {
        if self.is_frame() {
            // R1595 — a user-sized frame authors a HEIGHT, which is the one
            // node body whose height nothing derives.
            return ipx(self.appearance.height.unwrap_or(upx(FRAME_MIN)));
        }
        if self.is_reroute() {
            // R1243 — a reroute knot is a square dot: no header / port rows /
            // body pad, just the [`KNOT_SIZE`] the width also takes.
            return KNOT_SIZE;
        }
        let rows = match &self.body {
            NodeBody::Kind(op) => irow(op.inputs().len().max(op.outputs().len()).max(1)),
            _ => irow(1),
        };
        HEADER_H + rows * PORT_PITCH + BODY_PAD
    }

    fn right(&self) -> i32 {
        self.x + self.width()
    }

    fn bottom(&self) -> i32 {
        self.y + self.height()
    }

    fn is_reroute(&self) -> bool {
        matches!(self.body, NodeBody::Kind(MaterialOp::Reroute(_)))
    }

    fn title(&self) -> String {
        self.display_name()
    }
}

/// R948 — the union bounding box `(left, top, right, bottom)` in graph units
/// of a node set (each node spans `x..right()` × `y..bottom()`), or `None`
/// for an empty set. The one home for the min/max fold that `frame_all` (all
/// nodes) and `align_selected` (the selection) both need — a divergent fold
/// would let the two read a node's extent differently ([[ssot-lift-grep-repo-wide-cross-enum]]).
fn node_bounds<'a>(
    nodes: impl Iterator<Item = &'a Node<MaterialOp>>,
) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for n in nodes {
        bounds = Some(match bounds {
            None => (n.x, n.y, n.right(), n.bottom()),
            Some((l, t, r, b)) => (l.min(n.x), t.min(n.y), r.max(n.right()), b.max(n.bottom())),
        });
    }
    bounds
}

/// R1227 / R1596 — whether node `n`'s CENTRE lies inside `frame`'s rect.
///
/// Membership itself is **not** this: a node belongs to the frame its
/// `Node::parent` names, which the crate maintains as a forest. This is the
/// geometric question the *gestures* ask — which nodes a fresh frame should
/// adopt, and which one a dragged node lands in — the Blueprint rule that a node
/// belongs to the frame it sits in even if its card overhangs the border.
fn frame_contains(frame: &Node<MaterialOp>, n: &Node<MaterialOp>) -> bool {
    // R1243 — each axis uses the node's own extent, so a compact reroute knot's
    // centre is where its dot actually is.
    let cx = n.x + n.width() / 2;
    let cy = n.y + n.height() / 2;
    cx >= frame.x && cx <= frame.right() && cy >= frame.y && cy <= frame.bottom()
}

/// Live drag preview while a wire is being pulled — from an output port (a
/// fresh connection), or, during an R1174 *reconnect*, from an existing edge's
/// SOURCE output (a wired input was grabbed and pulled loose). Either way the
/// anchored end is an output and the loose end snaps to the hovered input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Preview {
    from_node: NodeId,
    from_port: usize,
    /// Currently-hovered input port the wire would snap to, if any.
    to: Option<(NodeId, usize)>,
    /// R1174 — set when this drag *reconnects* an existing edge (a wired input
    /// pulled loose): the edge whose target moves on release, committed through
    /// [`NodeGraphExternal::reconnect_edge`]. `None` for a fresh connect drag
    /// from an output port (the loose end becomes a brand-new edge instead).
    reconnect: Option<EdgeId>,
}

/// R1220 — the state of an open pin-drop create menu. `from_node` / `from_port`
/// are the output pin the wire was pulled from (its [`PortType`] gates the
/// type-filtered candidates + names the auto-wire source); `at_graph` is where
/// the created node lands (graph units, snapshotted at open so a later pan / zoom
/// does not move it); `at_screen` is the menu overlay's top-left in canvas px (it
/// paints OUTSIDE the world scroll, like the chrome). `filter` is the
/// type-to-narrow search text, `highlight` the roving active item **into the
/// filtered candidate list** (the Enter / arrow-key target).
// `Serialize`/`Deserialize`: satisfies the §2 #7 scene-as-data bound like
// [`Preview`] (transient UI state, never actually persisted).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PinCreate {
    from_node: NodeId,
    from_port: usize,
    at_graph: (i32, i32),
    at_screen: (u32, u32),
    filter: String,
    highlight: usize,
}

/// What is selected — a set of nodes, an edge, or nothing. A sum type so
/// "both nodes and an edge selected" is unrepresentable; the handles are
/// stable ids, so a selection survives an unrelated delete.
///
/// R879 — `Nodes` generalises the pre-R879 single `Node(NodeId)` to a
/// non-empty [`BTreeSet`] (the marquee / `Ctrl`-click substrate; an
/// unordered graph addresses by stable id, so the 1-D index+range
/// [`pinion_core::widgets::virtual_select`] coordinator is the wrong
/// abstraction here — the *policy* is mirrored, the repr is not). The
/// non-empty invariant is upheld by [`Selection::from_nodes`]: an empty
/// set collapses to `None`, so "selected but zero nodes" is also
/// unrepresentable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Selection {
    None,
    Nodes(BTreeSet<NodeId>),
    Edge(EdgeId),
}

impl Selection {
    /// A single-node selection.
    fn single(id: NodeId) -> Self {
        Selection::Nodes(BTreeSet::from([id]))
    }

    /// A node-set selection, collapsing an empty set to `None` (the
    /// non-empty invariant's single construction funnel).
    fn from_nodes(set: BTreeSet<NodeId>) -> Self {
        if set.is_empty() {
            Selection::None
        } else {
            Selection::Nodes(set)
        }
    }

    /// The selected node id **when exactly one node is selected** — the
    /// single-target operations' guard (rename / `query selected`): a
    /// multi-selection has no unambiguous "the" node.
    fn node(&self) -> Option<NodeId> {
        match self {
            Selection::Nodes(set) if set.len() == 1 => set.first().copied(),
            _ => None,
        }
    }

    /// The selected node set (empty for `None` / `Edge`).
    fn nodes(&self) -> BTreeSet<NodeId> {
        match self {
            Selection::Nodes(set) => set.clone(),
            _ => BTreeSet::new(),
        }
    }

    /// Whether `id` is a selected node.
    fn contains_node(&self, id: NodeId) -> bool {
        matches!(self, Selection::Nodes(set) if set.contains(&id))
    }

    /// The selected edge id, if an edge is selected.
    fn edge(&self) -> Option<EdgeId> {
        match self {
            Selection::Edge(id) => Some(*id),
            _ => None,
        }
    }
}

/// R1596 — the seed graph's nodes / wires, for the tests that state a fact about
/// the FIRST-PAINT graph.
///
/// Derivations over [`default_graph`] rather than the two hand-written lists the
/// seed used to be, so a test cannot assert against a seed the editor does not
/// actually start from.
#[cfg(test)]
fn default_nodes() -> Vec<Node<MaterialOp>> {
    kind_node_vec(&default_graph())
}

#[cfg(test)]
fn default_edges() -> Vec<Edge> {
    edges(&default_graph()).to_vec()
}

/// R849 / R1596 — the first id a fresh graph will mint, node and wire.
///
/// The editor derived these as `max(seed id) + 1` and carried them in two
/// counters; a tree mints its own, so this asks the seed graph what it will hand
/// out next. The answer is the same number and it now has one source.
#[cfg(test)]
fn first_dynamic_node_id() -> u32 {
    let mut probe = default_graph();
    let id = probe
        .add_node(TREE, NodeBody::Frame, 0, 0)
        .expect("the seed graph has a root tree");
    id.0
}

#[cfg(test)]
fn first_dynamic_edge_id() -> u32 {
    default_edges().last().map_or(0, |e| e.id.0 + 1)
}

/// First-paint graph — a tiny material graph (`Texture` × `Color` →
/// `Multiply` → `Output`). Node ids 0..=3, edge ids 0..=2.
///
/// R1596 — built by the crate's own edits, so the seed is exactly the state a
/// user could have reached by hand: `connect` type-checks each of the three
/// wires and mints their ids in order, and the id counters the editor used to
/// derive from this list are the tree's.
fn default_graph() -> Graph {
    let mut graph = Graph::new("material");
    // (palette kind, x, y): Texture(0) x Color(1) -> Multiply(2) -> Output(4).
    for (kind, x, y) in [(0, 40, 70), (1, 40, 210), (2, 250, 110), (4, 470, 150)] {
        add_palette_node(&mut graph, kind, x, y).expect("PALETTE kind in range");
    }
    for (from, to_node, to_port) in [(0, 2, 0), (1, 2, 1), (2, 3, 0)] {
        graph
            .connect(
                TREE,
                Socket::new(NodeId(from), 0),
                Socket::new(NodeId(to_node), to_port),
            )
            .expect("the seed graph is well-typed and acyclic");
    }
    graph
}

/// R1256 / R1596 — add a [`PALETTE`] node of `kind` at `(x, y)`, answering its
/// fresh id. The single palette-node constructor behind [`default_graph`] /
/// `add_node` / `commit_pin_create_kind`, so a kind's title and op live in
/// exactly one place. `None` for an out-of-range kind.
fn add_palette_node(graph: &mut Graph, kind: usize, x: i32, y: i32) -> Option<NodeId> {
    let &(title, op) = PALETTE.get(kind)?;
    let id = graph.add_node(TREE, NodeBody::Kind(op), x, y).ok()?;
    // The palette's title IS the op's name, so the label stays `None` and
    // `display_name` answers from the body — a rename is then the only thing
    // that can make the two differ, which is what a rename means.
    debug_assert_eq!(title, op.name());
    Some(id)
}

/// R852 / R1596 — the persistable graph snapshot: the schema version and the
/// [`Document`], whose serde carries the nodes, the wires, the frames, the
/// authored port values and the id counters together.
///
/// The editor's own snapshot carried six parallel fields (nodes, edges, frames
/// and three id counters) that had to be re-checked against each other on load;
/// a document is one value, and `Document::validate` is the check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SerializedGraph {
    schema_version: u32,
    graph: Graph,
}

// ─── geometry (window coordinates; canvas == window) ───────────────

/// The vertical offset of port row `row` within a node card (top of the
/// port box).
fn port_row_top(row: usize) -> i32 {
    HEADER_H + irow(row) * PORT_PITCH + PORT_PITCH / 2 - PORT_SIZE / 2
}

/// R1243 — the centre of a reroute knot in graph units: both its input and its
/// output port anchor here, so a wire passes straight through the dot (the
/// Blueprint reroute look). A reroute's `width()` and `height()` are both
/// [`KNOT_SIZE`], so this is the geometric centre of the painted dot.
fn knot_center(node: &Node<MaterialOp>) -> (i32, i32) {
    (node.x + node.width() / 2, node.y + node.height() / 2)
}

/// Centre of input port `i` of `node`, in window coordinates.
fn input_port_center(node: &Node<MaterialOp>, i: usize) -> (i32, i32) {
    if node.is_reroute() {
        // R1243 — a knot has no port rows; every incident wire anchors at its
        // centre, so the drawn wire terminates on the dot.
        return knot_center(node);
    }
    (
        node.x + PORT_SIZE / 2,
        node.y + port_row_top(i) + PORT_SIZE / 2,
    )
}

/// Centre of output port `j` of `node`, in window coordinates.
fn output_port_center(node: &Node<MaterialOp>, j: usize) -> (i32, i32) {
    if node.is_reroute() {
        return knot_center(node);
    }
    (
        node.right() - PORT_SIZE / 2,
        node.y + port_row_top(j) + PORT_SIZE / 2,
    )
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
fn cubic_at(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), t: f64) -> (f64, f64) {
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
    let t = if len2 <= 0.0 {
        0.0
    } else {
        ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (seg_a.0 + t * vx, seg_a.1 + t * vy);
    let (dx, dy) = (point.0 - cx, point.1 - cy);
    dx * dx + dy * dy
}

/// Whether `(px, py)` (graph units) lands within `threshold` of the wire
/// from `from` to `to` — the click-to-select-an-edge predicate. The curve
/// is sampled into [`EDGE_SAMPLES`] segments and the click tested against
/// each (a thin wire needs no analytic root-finding).
///
/// R877 — the test runs in graph space; the caller scales the
/// screen-constant [`EDGE_HIT_THRESHOLD`] by `1 / zoom` so the clickable
/// halo around a wire stays the same *on-screen* size at every zoom (at
/// 50% zoom a wire is half as wide, so its graph-space halo must be
/// twice as generous).
fn point_near_edge(px: f64, py: f64, from: (i32, i32), to: (i32, i32), threshold: f64) -> bool {
    let (c1, c2) = edge_curve(from, to);
    let click = (px, py);
    let p0 = (f64::from(from.0), f64::from(from.1));
    let p1 = (f64::from(c1.0), f64::from(c1.1));
    let p2 = (f64::from(c2.0), f64::from(c2.1));
    let p3 = (f64::from(to.0), f64::from(to.1));
    let thr2 = threshold * threshold;
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

/// The signed area of the triangle `a`-`b`-`c` (twice it): `> 0` when `c` is
/// left of the directed segment `a`->`b`, `< 0` right, `0` collinear. The
/// orientation primitive behind [`segments_cross`].
fn orient2(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Whether `c` (known collinear with `a`-`b`) lies within the segment `a`-`b`
/// (its axis-aligned bounding box) — the boundary leg of [`segments_cross`].
fn on_segment(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    c.0 >= a.0.min(b.0) && c.0 <= a.0.max(b.0) && c.1 >= a.1.min(b.1) && c.1 <= a.1.max(b.1)
}

/// Whether segments `p1`-`p2` and `p3`-`p4` intersect — the robust CLRS
/// predicate: a PROPER crossing (each segment straddles the other's supporting
/// line, both orientation pairs strictly opposite) OR a boundary touch (an
/// endpoint collinear with and inside the other segment). The boundary leg
/// matters because the wire is sampled into sub-segments whose vertices can land
/// exactly on the straight cut line — a strict-only test would then miss a knife
/// that slices cleanly through a sample vertex (a flat wire cut at its midpoint).
/// "Touching counts as cutting" is the intended knife behaviour.
fn segments_cross(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> bool {
    let d1 = orient2(p3, p4, p1);
    let d2 = orient2(p3, p4, p2);
    let d3 = orient2(p1, p2, p3);
    let d4 = orient2(p1, p2, p4);
    if (d1 > 0.0) != (d2 > 0.0)
        && d1 != 0.0
        && d2 != 0.0
        && (d3 > 0.0) != (d4 > 0.0)
        && d3 != 0.0
        && d4 != 0.0
    {
        return true;
    }
    (d1 == 0.0 && on_segment(p3, p4, p1))
        || (d2 == 0.0 && on_segment(p3, p4, p2))
        || (d3 == 0.0 && on_segment(p1, p2, p3))
        || (d4 == 0.0 && on_segment(p1, p2, p4))
}

/// R1226 — whether the straight cut segment `a`-`b` crosses the wire from
/// `from` to `to`. The curve is sampled into [`EDGE_SAMPLES`] sub-segments —
/// the SAME `edge_curve` + `cubic_at` control points [`point_near_edge`] reads —
/// and each sub-segment is tested against the cut with [`segments_cross`].
/// R1232 — the knife tests this 18-chord polyline approximation, which the paint
/// (a true `CurveTo` cubic) matches only up to the sub-pixel chord deviation;
/// unlike the click hit-test's `EDGE_HIT_THRESHOLD` halo, the crossing test has
/// no tolerance, so a cut grazing a bowed wire's bulge between two samples can
/// miss (or a concave pinch can false-cut) within that chord-vs-curve gap. Fine
/// at the pixel scale; an analytic segment-vs-cubic root would be exact.
fn edge_crosses_segment(from: (i32, i32), to: (i32, i32), a: (i32, i32), b: (i32, i32)) -> bool {
    let (c1, c2) = edge_curve(from, to);
    let p0 = (f64::from(from.0), f64::from(from.1));
    let p1 = (f64::from(c1.0), f64::from(c1.1));
    let p2 = (f64::from(c2.0), f64::from(c2.1));
    let p3 = (f64::from(to.0), f64::from(to.1));
    let ca = (f64::from(a.0), f64::from(a.1));
    let cb = (f64::from(b.0), f64::from(b.1));
    let mut prev = p0;
    for step in 1..=EDGE_SAMPLES {
        let t = f64::from(step) / f64::from(EDGE_SAMPLES);
        let cur = cubic_at(p0, p1, p2, p3, t);
        if segments_cross(prev, cur, ca, cb) {
            return true;
        }
        prev = cur;
    }
    false
}

/// R1238 — the hard right / bottom bound a node's top-left may take so the whole
/// card stays on the world surface (`WORLD` minus the card extent). Named once so
/// the per-node clamp ([`clamp_node_x`] / [`clamp_node_y`]) and the frame rigid-
/// group clamp ([`clamp_frame_dx`] / [`clamp_frame_dy`]) can never disagree on the
/// ceiling — the bare `WORLD - HEADER_H - 8` literal was duplicated across them.
const MAX_NODE_X: i32 = WORLD - NODE_W;
const MAX_NODE_Y: i32 = WORLD - HEADER_H - 8;

/// R877 — node positions clamp to the WORLD extent, not the window: the
/// canvas pans, so the old window-extent clamp would have pinned every
/// node inside the boot view. The unsigned scene substrate makes `0` the
/// world's hard left/top edge; the right/bottom clamp keeps the whole
/// card on the world surface.
fn clamp_node_x(x: i32) -> i32 {
    x.clamp(0, MAX_NODE_X)
}

fn clamp_node_y(y: i32) -> i32 {
    y.clamp(0, MAX_NODE_Y)
}

// ─── R877 viewport math (graph units ↔ world px ↔ canvas px) ───────
//
// One affine: `world = graph · zoom`, `canvas = world − scroll_offset`.
// Graph units are what the model stores (node x/y — identical to the
// pre-R877 coordinates, so saved graphs load unchanged); world px are
// what the view paints inside the scroll content; canvas px are what
// the cursor reports relative to the `GRAPH_TAG` rect.

/// Project a graph-unit length/coordinate into world px at `zoom`.
fn wpx(graph: i32, zoom: f64) -> i32 {
    round_i32(f64::from(graph) * zoom)
}

/// A stroke width scaled to `zoom`, floored at one visible pixel.
fn wstroke(width: u32, zoom: f64) -> u32 {
    upx(wpx(i32::try_from(width).unwrap_or(1), zoom).max(1))
}

/// A font size scaled to `zoom`, floored at a legible minimum.
fn wfont(px: u32, zoom: f64) -> u32 {
    upx(wpx(i32::try_from(px).unwrap_or(12), zoom).max(6))
}

/// R880 — normalise two graph-space corners into `(x0, y0, x1, y1)` with
/// `x0 <= x1`, `y0 <= y1`: the marquee's fixed press anchor + live cursor,
/// in either drag direction.
///
/// R880.1 — corners clamp to the world extent `[0, WORLD]`: a captured
/// cursor can stray off the canvas (negative / past-world graph coords),
/// and the paint path floors negatives at 0 (`upx`) while keeping the
/// size — an unclamped rect would paint a band wider than the area the
/// release applies. Clamping HERE keeps the painted band and the rect-hit
/// area one value (nodes live in `[0, WORLD]`, so the hit set is
/// unchanged).
fn corner_rect(a: (f64, f64), b: (f64, f64)) -> MarqueeRect {
    let world = f64::from(WORLD);
    let cl = |v: f64| round_i32(v.clamp(0.0, world));
    (
        cl(a.0.min(b.0)),
        cl(a.1.min(b.1)),
        cl(a.0.max(b.0)),
        cl(a.1.max(b.1)),
    )
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

/// R1596 — the graph: one [`Document`] where the editor kept five cells.
///
/// `use_nodes`, `use_edges`, `use_frames`, `use_next_node_id`,
/// `use_next_edge_id` and `use_next_frame_id` were six reactive holders whose
/// consistency nobody maintained — a node list that could name a missing edge
/// target, a counter that could hand out a live id. A document holds all of it
/// and is the thing `Document::validate` is a statement about.
#[must_use]
fn use_document() -> Rc<Signal<Graph>> {
    let owner = Owner::current().expect("use_document requires an active Owner scope");
    owner.cache("node_graph.document", || Signal::new(default_graph()))
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

/// R1220 — the in-flight pin-drop create menu (`None` when closed): the signature
/// the engine / the DCC authoring gesture — drag a wire off an output pin,
/// release on empty canvas, and a type-filtered menu of the nodes that output
/// can feed opens; pick one and it is created at the drop point AND auto-wired
/// in one undo step. Written by the coordinator (the live [`NodeGraphExternal::drag_release`] empty-canvas
/// branch and the `open_pin_create` RPC verb), read by the view fn (which paints the
/// floating menu — reading it subscribes the paint Effect, so open / filter /
/// highlight changes repaint) and the keyboard path. Transient UI state, like
/// [`use_preview`] / [`use_marquee_rect`].
#[must_use]
fn use_pin_create() -> Rc<Signal<Option<PinCreate>>> {
    let owner = Owner::current().expect("use_pin_create requires an active Owner scope");
    owner.cache("node_graph.pin_create", || Signal::new(None))
}

/// R880 — the live marquee rectangle in graph units, as normalised corners
/// `(x0, y0, x1, y1)` (`None` while no marquee is in flight). Written by the
/// coordinator's background-drag path once the press latches past the
/// click-vs-drag dead zone; read by the view fn (which paints the rubber
/// band — reading it subscribes the paint Effect, so every drag step
/// repaints). Transient UI state, like [`use_preview`].
#[must_use]
fn use_marquee_rect() -> Rc<Signal<Option<MarqueeRect>>> {
    let owner = Owner::current().expect("use_marquee_rect requires an active Owner scope");
    owner.cache("node_graph.marquee", || Signal::new(None))
}

/// R901 — what the ONE shared inline field is editing (`None` when idle).
/// Generalises the R878 single-purpose `renaming: Option<NodeId>` so the same
/// field, focus, blur-commit, keymap, and a11y machinery drive two edit
/// targets: a node **title** (R878) and an input **port default** (R899). The
/// field paints inside a node card either way (a title swaps the header, a
/// port default swaps the pin's default label), so both targets name the
/// hosting node ([`EditTarget::node`]).
// `Serialize`/`Deserialize`: the `Signal<Option<EditTarget>>` holder satisfies
// the §2 #7 scene-as-data bound (like `Signal<Option<NodeId>>` before it),
// though this transient UI state is never actually persisted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum EditTarget {
    /// Editing node `id`'s title (the R878 rename).
    Title(NodeId),
    /// Editing node `node`'s input port `port`'s literal default value (R901).
    PortDefault { node: NodeId, port: usize },
    /// R918 — editing node `id`'s `x` position (the Details panel's "Position X"
    /// row). Panel-only: a node card has no position field to swap, so this
    /// target is only ever opened with [`EditSurface::Panel`].
    PosX(NodeId),
    /// R918 — editing node `id`'s `y` position (the Details panel's "Position Y"
    /// row). Panel-only, like [`EditTarget::PosX`].
    PosY(NodeId),
    /// R1264 — editing node `id`'s authored output constant (a **source** node's
    /// [`source_const`], the output-side twin of the
    /// [`EditTarget::PortDefault`] pin default). Card-only: the constant paints
    /// on the source card's output row, which is the only surface hosting the
    /// field. The write funnels through the same [`apply_set_node_value`] /
    /// [`NodeValueTarget::OutputConst`] SSOT the AI-first `intervene
    /// node.<id>.value` uses (R1257).
    SourceConst(NodeId),
}

impl EditTarget {
    /// The node whose property this target edits. The migration-commit, the
    /// paint switch, and the a11y child all key off this id.
    fn node(self) -> NodeId {
        match self {
            EditTarget::Title(id)
            | EditTarget::PortDefault { node: id, .. }
            | EditTarget::PosX(id)
            | EditTarget::PosY(id)
            | EditTarget::SourceConst(id) => id,
        }
    }
}

/// R918 — which surface hosts the ONE shared inline field while an edit is in
/// flight. The card (R878 header title / R901 pin default) and the Details panel
/// (R916) both edit a single selected node's properties through the *same*
/// field, focus, keymap, blur-commit, and undo funnel; this names which one
/// paints the field (and hosts its a11y textbox child) so the field — unique by
/// its [`EDIT_TF_TAG`] tag — renders in exactly one place. Orthogonal to *what*
/// is edited ([`EditTarget`]): a title is editable from either surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum EditSurface {
    /// The node card in the canvas — a header title (R878) or a pin default
    /// label (R901).
    Card,
    /// A Details panel row (R918) — the engine "click a property to edit it"
    /// surface.
    Panel,
}

/// R918 — the in-flight inline edit: *what* is being edited ([`EditTarget`]) and
/// on *which* surface ([`EditSurface`]). Generalises the R878/R901
/// `Option<EditTarget>` so the same field machinery drives both the node card
/// and the Details panel without a second field. `None` (the holder's idle
/// state) means no edit is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct ActiveEdit {
    target: EditTarget,
    surface: EditSurface,
}

/// R878 / R901 / R918 — the in-flight inline edit (`None` when none is open):
/// *what* is being edited and on *which* surface ([`ActiveEdit`]). Shared by the
/// coordinator (begin / commit), the view fn (paints the field over the card
/// title / pin default or the Details panel row), and the keyboard / blur-intent
/// commit paths. Transient UI state — never persisted.
fn use_active_edit() -> Rc<Signal<Option<ActiveEdit>>> {
    let owner = Owner::current().expect("use_active_edit requires an active Owner scope");
    owner.cache("node_graph.editing", || Signal::new(None))
}

// R1596 — the three monotonic id `Cell`s are gone: a `Tree` mints node and link
// ids itself, so a minted id being unique is a property of the model rather than
// of three counters a save/load path had to carry and re-seed in step.

/// R877 — the canvas zoom (shared coordinator ↔ view): the view fn projects
/// every world coordinate through it; the coordinator's `Ctrl`+wheel /
/// keyboard / `intervene viewport.zoom` paths write it. Transient UI state
/// (like the selection), so it is not persisted.
#[must_use]
fn use_zoom() -> Rc<Signal<f64>> {
    let owner = Owner::current().expect("use_zoom requires an active Owner scope");
    owner.cache("node_graph.zoom", || Signal::new(1.0))
}

/// R877 — the canvas pan: the [`ScrollState`] behind the
/// [`ScrollAxis::Both`] world scroll. One `Rc` shared by the view (the
/// `ScrollNode` link), the coordinator (anchored zoom + `viewport.x/y`
/// intervene), and the router's native wheel dispatch — pan IS scroll, so
/// the plain-wheel path needs no canvas-specific code at all.
#[must_use]
fn use_canvas_scroll() -> Rc<ScrollState> {
    let owner = Owner::current().expect("use_canvas_scroll requires an active Owner scope");
    owner.cache("node_graph.scroll", ScrollState::new)
}

/// R1182 — the node-drag latch (members + dead-zone [`DragLatch`]), shared as an
/// `Rc` so the [`AutoPan`] tick reads the SAME authoritative drag state the
/// coordinator writes (one source of truth, no per-frame copy). The coordinator
/// takes this holder through [`GraphServices`]; the auto-pan driver clones it.
fn use_node_drag() -> Rc<RefCell<Option<NodeDragStart>>> {
    let owner = Owner::current().expect("use_node_drag requires an active Owner scope");
    owner.cache("node_graph.node_drag", || RefCell::new(None))
}

/// R1182 — register the edge-drag auto-pan driver on the owner's animation
/// clock (idempotent; the `use_caret_blink` *register-once mechanism* is the
/// precedent — the rest-semantics differ, see [`AutoPan::is_at_rest`]). Called
/// from the view setup so it ticks on every paint cycle, self-gating via
/// [`AutoPan::active`].
fn use_autopan() {
    let owner = Owner::current().expect("use_autopan requires an active Owner scope");
    let document = use_document();
    let scroll = use_canvas_scroll();
    let zoom = use_zoom();
    let node_drag = use_node_drag();
    let _: Rc<AutoPan> = owner.register_animation_once("node_graph.autopan", move || AutoPan {
        document,
        scroll,
        zoom,
        node_drag,
    });
}

/// R1183 — whether an auto-pan `push` along one axis can still move the
/// viewport, given the axis `offset` and its scroll `max`. A push toward an
/// edge whose offset is already pinned at the bound (`0` for a negative push,
/// `max` for a positive push) makes no progress — the [`ScrollState`] clamp
/// would no-op — so that axis is at rest. Pure so it is unit-testable without
/// building an `AutoPan`.
fn autopan_axis_can_move(push: f64, offset: i32, max: i32) -> bool {
    (push < 0.0 && offset > 0) || (push > 0.0 && offset < max)
}

/// R1182 — edge-drag auto-pan: while a node drag is held against the canvas rim,
/// scroll the viewport toward that edge each frame and keep the dragged nodes
/// pinned under the cursor. Registered as a framework [`Tickable`]
/// ([`use_autopan`]). It reads the drag's own [`NodeDragStart::cursor`] probe +
/// latch (the authoritative drag state — one source of truth for "which nodes,
/// gripped where, under the cursor now"), so there is no separate cursor holder
/// to keep in lifecycle-sync.
struct AutoPan {
    document: Rc<Signal<Graph>>,
    scroll: Rc<ScrollState>,
    zoom: Rc<Signal<f64>>,
    node_drag: Rc<RefCell<Option<NodeDragStart>>>,
}

impl AutoPan {
    /// `(x_rel, y_rel, push_x, push_y)` when a live *latched* node drag sits in
    /// the rim **and** the viewport still has scroll headroom in the push
    /// direction, else `None` — a marquee / dead-zone press / centred drag /
    /// idle canvas / a rim already pinned at the world edge all rest. The one
    /// gate `tick` and `is_at_rest` share (so the driver never burns frames
    /// once panning can make no further progress — the R1183 clamp-rest fix).
    fn active(&self) -> Option<(f64, f64, f64, f64)> {
        let drag = self.node_drag.borrow();
        let start = drag.as_ref().filter(|s| s.latch.live())?;
        let (x_rel, y_rel) = start.cursor.get();
        let (px, py) = (autopan_push(x_rel), autopan_push(y_rel));
        let (ox, oy) = self.scroll.offset();
        let (mx, my) = self.scroll.max();
        let can_pan = autopan_axis_can_move(px, ox, mx) || autopan_axis_can_move(py, oy, my);
        can_pan.then_some((x_rel, y_rel, px, py))
    }
}

impl Tickable for AutoPan {
    fn tick(&self, dt: f32) {
        let Some((x_rel, y_rel, px, py)) = self.active() else {
            return;
        };
        let dt = f64::from(dt);
        // Scroll the viewport toward the rim (the `ScrollState` clamps to the
        // world extent — an axis already at the world edge simply stops, and
        // `active` has already reported at-rest once *both* axes are pinned).
        self.scroll.scroll_by(
            round_i32(px * AUTOPAN_SPEED * dt),
            round_i32(py * AUTOPAN_SPEED * dt),
        );
        // Keep the dragged nodes pinned under the (still) cursor against the new
        // viewport — the same re-derivation `pointer_move` runs on cursor
        // motion, here driven by the viewport moving under a stationary cursor.
        let (gx, gy) = cursor_graph_at(&self.scroll, self.zoom.get(), x_rel, y_rel);
        if let Some(start) = self.node_drag.borrow().as_ref() {
            follow_members(&self.document, &start.members, gx, gy);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // Progress-bounded, NOT inherently periodic like the caret blink: rest
        // as soon as the drag leaves the rim / releases OR the viewport can no
        // longer scroll in the push direction, so the backend's `request_redraw`
        // loop can idle instead of spinning on a pinned-at-the-edge hold.
        self.active().is_none()
    }
}

/// R851 — the shared [`UndoStack`] for this editor scope. Reached identically
/// by the coordinator (which records [`GraphEdit`]s in its mutation methods),
/// the keyboard `Ctrl+Z` path, the status-line `undo_label` read, and the
/// [`UndoStackExternal`] — one history source of truth ([`use_undo_stack`]).
fn use_undo() -> Rc<UndoStack> {
    use_undo_stack(UNDO_KEY)
}

/// R852/R857 — the shared graph store. The coordinator (which `save`s / `load`s)
/// is the only consumer today. R857: the boxed-dyn + env-override + in-memory-
/// fallback cluster this once hand-rolled was lifted to
/// [`pinion_platform_storage::use_app_storage`] (a verified 3-copy across
/// todomvc / settings-panel / this example — the `use_app_clipboard` R790
/// precedent); this is now a thin call into that SSOT.
fn use_graph_storage() -> Rc<AppStorage> {
    use_app_storage(STORAGE_CACHE_KEY, STORAGE_APP_NAME)
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

/// R1220 — `node_graph#create_<idx>` → the [`PALETTE`] index of a pin-drop
/// create-menu item. The [`parse_palette_sub`] peer over the `create_` prefix:
/// a menu-card click routes to `handle_send`, which commits that kind through
/// [`NodeGraphExternal::commit_pin_create_kind`] (the auto-wire path), never the
/// bare [`NodeGraphExternal::add_node`] a palette card runs.
fn parse_create_sub(sub: &str) -> Option<usize> {
    sub.strip_prefix("create_")?.parse().ok()
}

/// R918 — `node_graph#detail_<field>` → the Details-panel property key (`title`,
/// `x`, `y`, `in_<port>`). A click on a panel row routes to the coordinator (the
/// palette precedent — a view sibling tagged with the primary's prefix), which
/// opens the inline editor on the selected node's matching property.
fn parse_detail_sub(sub: &str) -> Option<&str> {
    sub.strip_prefix("detail_")
}

/// R918 — a Details-panel field key → the [`EditTarget`] it edits on node `id`
/// (the inverse of the `detail_<key>` row tags / `view_details_panel` keys).
/// `None` for an unknown key. A port's existence is validated later by
/// [`NodeGraphExternal::edit_seed_text`], so this stays a pure string map.
fn detail_edit_target(id: NodeId, field: &str) -> Option<EditTarget> {
    match field {
        "title" => Some(EditTarget::Title(id)),
        "x" => Some(EditTarget::PosX(id)),
        "y" => Some(EditTarget::PosY(id)),
        _ => field
            .strip_prefix("in_")?
            .parse()
            .ok()
            .map(|port| EditTarget::PortDefault { node: id, port }),
    }
}

/// R901.1 (session-review audit) — the `"<id>_<index>"` → (node id, port
/// index) underscore-split core shared by every composite pin sub-tag parser
/// (`oport_` / `idefault_` / `iport_`) and the drag-source decode. The prefix
/// strip is the caller's concern; this is the `_`-split → typed-pair SSOT (the
/// Rule-of-Three lift of the parser family that R901's `idefault_` sibling
/// pushed past three byte-identical copies).
fn split_node_port(s: &str) -> Option<(NodeId, usize)> {
    let (n, i) = s.split_once('_')?;
    Some((NodeId(n.parse().ok()?), i.parse().ok()?))
}

/// `oport_<id>_<j>` → (node id, output port).
fn parse_oport_sub(sub: &str) -> Option<(NodeId, usize)> {
    split_node_port(sub.strip_prefix("oport_")?)
}

/// R1174 — `iport_<id>_<i>` → (node id, input port): the bare send-wire peer of
/// [`parse_input_port_tag`] (which first strips the `node_graph#` drop-tag
/// prefix). A press on an input port square delivers this sub on the `send`
/// wire; a *wired* input arms the reconnect drag in
/// [`begin_drag`](NodeGraphExternal::begin_drag). The [`parse_oport_sub`] peer
/// over the `iport_` prefix.
fn parse_iport_sub(sub: &str) -> Option<(NodeId, usize)> {
    split_node_port(sub.strip_prefix("iport_")?)
}

/// R901 — `idefault_<id>_<i>` → (node id, input port): the pin-default label's
/// composite sub-tag (the [`view_pin_value_label`] hit target). Double-clicking
/// it opens the inline default editor (the [`parse_oport_sub`] peer over the
/// `idefault_` prefix).
fn parse_idefault_sub(sub: &str) -> Option<(NodeId, usize)> {
    split_node_port(sub.strip_prefix("idefault_")?)
}

/// R1264 — `oconst_<id>` → node id: a **source** node's output-constant label
/// (the [`view_pin_value_label`] hit target on the output row). Double-clicking
/// it opens the inline source-value editor — the output-side twin of the R901
/// `idefault_` pin-default gesture. One id, no port index (the constant is
/// output port 0's), so it does not go through [`split_node_port`].
fn parse_oconst_sub(sub: &str) -> Option<NodeId> {
    Some(NodeId(sub.strip_prefix("oconst_")?.parse().ok()?))
}

/// A full drop tag `node_graph#iport_<id>_<i>` → (node id, input port). Uses
/// the canonical `#` splitter (`split_subindex`) rather than an inline split.
fn parse_input_port_tag(tag: &str) -> Option<(NodeId, usize)> {
    split_node_port(split_subindex(tag).1?.strip_prefix("iport_")?)
}

/// Two comma-separated `i32`s ("dx,dy"). R1451 — the shared typed-pair codec.
fn parse_pair_i32(s: &str) -> Option<(i32, i32)> {
    parse_typed_pair(s, ',')
}

/// Join stable ids into the CSV the `node_ids` / `edge_ids` queries return.
fn csv_ids(ids: impl Iterator<Item = u32>) -> String {
    ids.map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

/// R898 — join a port's types into the CSV the `node.<id>.input_types` /
/// `output_types` queries return ("Vector,Float"; "" for a source / sink).
fn port_types_csv(ports: &[Port<PortType, CellValue>]) -> String {
    ports
        .iter()
        .map(|p| p.value_type().map_or(CONTROL_PORT, |t| t.name()))
        .collect::<Vec<_>>()
        .join(",")
}

/// R901 — `"<node>.<port>"` → (node id, input port). The `begin_edit_default`
/// invoke arg, mirroring the `node.<id>.input_default.<port>` path's dotted
/// `<node>.<port>` addressing.
fn parse_node_port(s: &str) -> Option<(NodeId, usize)> {
    let (node, port) = s.split_once('.')?;
    Some((NodeId(node.trim().parse().ok()?), port.trim().parse().ok()?))
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

/// R929 — parse the `reconnect_edge` arg `"edge,to_node,to_port"`: the edge to
/// re-wire and its new target input. The source output is read from the edge
/// itself, so only three fields cross the wire (vs [`parse_quad`]'s four).
fn parse_reconnect(csv: &str) -> Option<(EdgeId, NodeId, usize)> {
    let parts: Vec<&str> = csv.split(',').collect();
    let [edge, tnode, tport] = parts.as_slice() else {
        return None;
    };
    Some((
        EdgeId(edge.trim().parse().ok()?),
        NodeId(tnode.trim().parse().ok()?),
        tport.trim().parse().ok()?,
    ))
}

/// R1226 — parse the `cut_wires` arg `"x1,y1,x2,y2"` (a straight cut segment in
/// graph units) into its two endpoints. `None` on a malformed spec, so the verb
/// Rejects rather than cutting nothing silently.
fn parse_cut_spec(csv: &str) -> Option<((i32, i32), (i32, i32))> {
    let parts: Vec<&str> = csv.split(',').collect();
    let [x1, y1, x2, y2] = parts.as_slice() else {
        return None;
    };
    Some((
        (x1.trim().parse().ok()?, y1.trim().parse().ok()?),
        (x2.trim().parse().ok()?, y2.trim().parse().ok()?),
    ))
}

// ─── internal drag latches (Cell — not read by the view) ───────────

/// Snapshot taken on the first capture move of a node drag.
///
/// R877 — the anchor is the *graph-space grab offset* (`node_pos −
/// cursor_graph` at the press), not a cursor-fraction delta: every later
/// move re-derives `node_pos = cursor_graph + grab` against the *current*
/// viewport, so a zoom or pan mid-drag (`Ctrl`+wheel while holding a card)
/// keeps the grab point pinned under the cursor instead of drifting — the
/// canonical anchor model. `*_at_press` feeds the R853 one-move-per-gesture
/// undo journal.
#[derive(Clone, Debug)]
struct NodeDragStart {
    /// R879 — one entry per dragged member: `(id, grab_dx, grab_dy,
    /// x_at_press, y_at_press)`. Grabbing a *selected* node drags the whole
    /// selection rigidly (each member keeps its own graph-space grab
    /// anchor); grabbing an unselected node drags just it (the engine /
    /// canvas view group-move convention). Id-sorted by construction
    /// (selection-set order) so `end_gesture`'s journal entry matches
    /// `GraphEdit::merge`'s same-member ordering.
    members: Vec<(NodeId, f64, f64, i32, i32)>,
    /// R879 audit fix / R880 — the dead zone: the framework [`DragLatch`]
    /// (the SAME contract predicate the router and the marquee advance).
    /// Until it latches, members do NOT move (the toolkit `startDragDistance`
    /// dead zone — a jittery click neither displaces nodes nor journals a
    /// move) and the release still selects (`gesture_moved` reads this
    /// SAME latch, so "the nodes moved" and "the release must not select"
    /// can never disagree).
    latch: DragLatch,
    /// R1183 — the last canvas cursor fraction of this drag (the [`AutoPan`]
    /// rim probe). Co-located with the drag it belongs to (R1182 held it in a
    /// separate `autopan_cursor` holder whose lifecycle had to be cleared by
    /// hand at every `node_drag = None` site — this ties the probe to the
    /// drag by construction so it can never go stale). `Cell` so `pointer_move`
    /// updates it through the shared `&` borrow.
    cursor: Cell<(f64, f64)>,
}

/// R880 — a marquee rectangle in graph units as normalised corners
/// `(x0, y0, x1, y1)` (`x0 <= x1`, `y0 <= y1`).
type MarqueeRect = (i32, i32, i32, i32);

/// R880 — snapshot taken on the capture seed of a *background* press (the
/// marquee gesture's twin of [`NodeDragStart`]). The press anchor is held in
/// both metric spaces: screen px for the [`DragLatch`] dead-zone test (the
/// same framework contract predicate the router and the node drag advance)
/// and graph units for the rubber-band rect (so a pan / zoom mid-marquee
/// keeps the anchor pinned to the world, not the glass — the R877 anchor
/// model).
#[derive(Clone, Copy, Debug)]
struct MarqueeStart {
    /// The dead zone over the press cursor in screen px — the framework
    /// [`DragLatch`] (the SAME contract predicate the router and the node
    /// drag advance). The single predicate (R879.1 one-gate principle) for
    /// both "paint the rubber band" and "the release applies the rect, not
    /// the edge-click probe".
    latch: DragLatch,
    /// The press cursor in graph units — the rubber band's fixed corner.
    press_graph: (f64, f64),
}

/// R1592 — a card's extent for an area selection, as **inclusive** corners in
/// graph units: `x..=right()`, its far edge INCLUDED.
///
/// A card spans `x..right()` everywhere else in this file, so this widens it by one
/// unit on each far side — deliberately, and it is the "touching counts" rule
/// this marquee has stated since R880 (the toolkit's rubber band and the
/// engine's share it): a sweep whose edge lands exactly on a card's far edge
/// takes the card. Before R1592 that rule lived as `n.right() >= x0` inside a filter, where
/// it read as an off-by-one; here it is a sentence about the card, and the one
/// place the marquee, the lasso and the circle all get it from.
fn card_span(node: &Node<MaterialOp>) -> (Point, Point) {
    (
        Point::new(node.x.into(), node.y.into()),
        Point::new(node.right().into(), node.bottom().into()),
    )
}

/// R880.1 — flip `id`'s membership in a node set (the toggle kernel the
/// `Ctrl`-click and the `Ctrl`-marquee share — one definition so the two
/// chord consumers cannot drift).
fn toggle_member(set: &mut BTreeSet<NodeId>, id: NodeId) {
    if !set.remove(&id) {
        set.insert(id);
    }
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
    /// R1174 — a press on an input port square, carrying the pressed
    /// `(node, port)`. A *wired* input arms a reconnect drag in
    /// [`begin_drag`](NodeGraphExternal::begin_drag) (the existing edge's target
    /// is pulled loose, committed through `reconnect_edge` on release); an
    /// unwired input has no edge to grab, so it stays a non-drag press that only
    /// suppresses the background edge-probe. Before R1174 this was a unit variant
    /// that discarded the identity — the missing half that left reconnect
    /// verb-only.
    InputPort(NodeId, usize),
    /// R850 — a press on an add-node palette card, carrying the pressed
    /// [`PALETTE`] index. It suppresses the background edge-probe (like
    /// [`Inert`](Self::Inert)), and R1411 makes it a DRAG SOURCE: the index lets
    /// [`begin_drag`](NodeGraphExternal::begin_drag) arm a drag-to-instantiate
    /// session so a card dropped on the canvas creates that node at the drop
    /// point, while a press-release IN PLACE still adds it at the spawn point on
    /// the matching `PointerUp`. Recorded as its own variant so neither release
    /// path is ever misread as an inert press.
    Palette(usize),
    /// R918 — a press on a Details-panel property row. A non-drag press whose
    /// inline-editor activation runs on the matching `PointerUp` (the palette
    /// precedent); recorded as its own variant so it is never mistaken for an
    /// inert or empty-canvas press.
    DetailRow,
    /// R1220 — a press on a pin-drop create-menu card. Like [`Palette`](Self::Palette)
    /// it is a non-drag press with a specific `PointerUp` action (it commits that
    /// kind through the auto-wire path, not the bare `add_node` a palette card
    /// runs), recorded as its own variant so the release never misreads it.
    CreateMenu,
    /// A non-drag press on an in-card target that is neither draggable nor has a
    /// release action — currently an unwired pin's default-value label. It only
    /// needs to suppress the background edge-probe (a press inside a node is not
    /// an empty-canvas click), so it carries no identity. Distinct from the named
    /// variants above, which each have specific drag / release behaviour.
    Inert,
}

// ─── reversible structural edit (the UndoCommand) ──────────────────

/// R851 §5.52 / R1596 — one reversible edit: the document before it and the
/// document after, plus the selection each way.
///
/// **This replaces four commands** — a granular `GraphEdit` delta, a
/// `MoveNodesCmd`, a `RenameCmd<T>` and a `SetNodeValueCmd` — and the reason is
/// not brevity: a delta had to enumerate every *kind* of thing an edit could
/// touch, and every round that gave the model a new fact had to remember to add
/// it. R1227 added comment frames and the delta grew two fields; R1594's authored
/// port values and R1589's containment would each have needed another, and an
/// edit that forgot one would journal an undo that silently lost it. A document
/// is one value, so there is nothing to enumerate.
///
/// The editor's own note called snapshots the wrong shape
/// ([[granular-undo-not-snapshot]]), against a model that was a `Vec<GraphNode>` the whole
/// graph had to be cloned out of by hand. It is also what **the DCC** does —
/// `node_undosys` stores a copy of the tree per step — and what makes an undo here
/// provably total rather than total-as-far-as-anyone-remembered.
struct GraphEdit {
    document: Rc<Signal<Graph>>,
    selection: Rc<Signal<Selection>>,
    label: Cow<'static, str>,
    before: Graph,
    after: Graph,
    sel_before: Selection,
    sel_after: Selection,
    /// R856 — the gesture a contiguous run folds along, or `None` for an edit
    /// that is always its own undo step.
    ///
    /// A keyboard nudge burst collapses to one step; a drag (already the whole
    /// gesture) and every discrete RPC edit do not. The key is what has to
    /// MATCH for two steps to be one gesture — the moved node set — so nudging
    /// a different selection starts a fresh step.
    coalesce: Option<BTreeSet<NodeId>>,
}

impl GraphEdit {
    /// Put the document and the selection into one of the two recorded states.
    ///
    /// R1596 — the restored document takes on the id frontier of the state it is
    /// LEAVING ([`Document::advance_ids_from`]), so an undo never hands a fresh
    /// node the id an undone one had. A snapshot restores its mint counters
    /// along with everything else, and the R838 stable-id model is what an
    /// agent, a saved selection and every scene tag address a node BY — reuse
    /// would let one id silently name two different nodes across an undo.
    fn restore(&self, graph: &Graph, sel: &Selection, leaving: &Graph) {
        let mut next = graph.clone();
        next.advance_ids_from(leaving);
        batch(|| {
            self.document.set(next);
            self.selection.set(sel.clone());
        });
    }
}

impl UndoCommand for GraphEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.restore(&self.after, &self.sel_after, &self.before);
    }

    fn undo(&self) {
        self.restore(&self.before, &self.sel_before, &self.after);
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }

    /// R856 — fold a contiguous same-gesture edit into this one: keep this
    /// step's `before` and take the successor's `after`.
    ///
    /// Contiguity is checked as *document equality* (`self.after ==
    /// next.before`), which is the same guard the per-node delta used — every
    /// caller captures `before` from live state, so a run is contiguous by
    /// construction and this keeps a future stale-`before` caller from folding a
    /// run and losing the intermediate state on undo.
    fn merge(&mut self, next: &dyn UndoCommand) -> bool {
        let Some(next) = next.as_any().and_then(|a| a.downcast_ref::<GraphEdit>()) else {
            return false;
        };
        let (Some(mine), Some(theirs)) = (self.coalesce.as_ref(), next.coalesce.as_ref()) else {
            return false;
        };
        if mine != theirs || self.after != next.before {
            return false;
        }
        self.after = next.after.clone();
        self.sel_after = next.sel_after.clone();
        self.label.clone_from(&next.label);
        true
    }
}

/// R1596 — everything one reversible edit touches: the document, the selection
/// it may move, and the history it lands on.
///
/// A [`GraphEdit`] restores BOTH holders, so any caller able to change one has
/// to name the other even when it does not touch it. Bundling them is what lets
/// the coordinator and the *view-side* commit paths (an inline field's blur,
/// which has no coordinator to reach through) run the SAME funnel instead of
/// each carrying its own argument list — the editor kept four command types
/// partly because a free function could not reach the coordinator's fields.
#[derive(Clone)]
struct GraphHandle {
    document: Rc<Signal<Graph>>,
    selection: Rc<Signal<Selection>>,
    undo: Rc<UndoStack>,
}

/// R1596 — the handle assembled from the shared reactive holders, for the paint
/// paths that have no [`NodeGraphExternal`] in hand.
fn use_graph_handle() -> GraphHandle {
    GraphHandle {
        document: use_document(),
        selection: use_selection(),
        undo: use_undo(),
    }
}

impl GraphHandle {
    /// The document as it stands.
    fn graph(&self) -> Graph {
        self.document.get()
    }

    /// R1596 — the ONE structural mutation path: run `mutate` on a copy of the
    /// document, journal the before/after pair, and let the undo stack apply it.
    ///
    /// `mutate` may change the document, the selection, or both; an edit that
    /// changes neither is journalled as nothing and answers `false`, which is
    /// the no-op discipline every one of the four commands this replaced kept
    /// separately. `coalesce` names the gesture a contiguous run folds along
    /// ([`GraphEdit::coalesce`]).
    fn edit(
        &self,
        label: impl Into<Cow<'static, str>>,
        coalesce: Option<BTreeSet<NodeId>>,
        mutate: impl FnOnce(&mut Graph, &mut Selection),
    ) -> bool {
        let before = self.graph();
        let sel_before = self.selection.get();
        let mut after = before.clone();
        let mut sel_after = sel_before.clone();
        mutate(&mut after, &mut sel_after);
        if after == before && sel_after == sel_before {
            return false;
        }
        self.undo.record(GraphEdit {
            document: Rc::clone(&self.document),
            selection: Rc::clone(&self.selection),
            label: label.into(),
            before,
            after,
            sel_before,
            sel_after,
            coalesce,
        });
        true
    }
}

/// R948 — which edge / centre line of the selection's bounding box an `align_*`
/// snaps every selected node to. Horizontal specs move only `x`, vertical
/// specs only `y` — the canonical the toolkit / the DCC / the design tool
/// align set.
#[derive(Clone, Copy)]
enum AlignSpec {
    Left,
    CenterH,
    Right,
    Top,
    CenterV,
    Bottom,
}

/// R948 — the axis a `distribute_*` equalises: it spaces the selected nodes'
/// CENTRES evenly along this axis between the two extreme centres.
#[derive(Clone, Copy)]
enum DistributeAxis {
    Horizontal,
    Vertical,
}

/// R948 — a node's centre coordinate along `axis` (the distribute sort key +
/// the spacing target), in graph units.
fn centre_key(n: &Node<MaterialOp>, axis: DistributeAxis) -> i32 {
    // The DOUBLED centre (`2x + w`), not `x + w/2`: an integer halving would
    // lose a unit on every odd extent, and the distribute pass recovers a
    // position as `(key - extent) / 2`, which is exact only on this form.
    match axis {
        DistributeAxis::Horizontal => 2 * n.x + n.width(),
        DistributeAxis::Vertical => 2 * n.y + n.height(),
    }
}

/// R1234 / R1240 / R1596 — the feasible one-axis translation `dx` of frame
/// `frame` keeping its whole RECT (`x .. right()`) AND every member node on the
/// world surface: the requested delta clamped to an interval `[lo, hi]` that
/// always contains `0` (the current state is on-world). A *rigid* group clamp —
/// the frame and its contents move by the SAME delta, so their relative geometry
/// is preserved even at the world edge (a node can never slide out of the frame
/// it sits in). `hi` bounds the frame's RIGHT edge, so an empty frame cannot be
/// pushed off the world. `hi.max(lo)` guards a pathologically world-wide frame
/// from an inverted range.
fn clamp_frame_dx(frame: &Node<MaterialOp>, members: &[Node<MaterialOp>], dx: i32) -> i32 {
    let mut lo = -frame.x;
    let mut hi = WORLD - frame.right();
    for n in members {
        lo = lo.max(-n.x);
        hi = hi.min(MAX_NODE_X - n.x);
    }
    dx.clamp(lo, hi.max(lo))
}

/// R1234 / R1240 — the vertical twin of [`clamp_frame_dx`].
fn clamp_frame_dy(frame: &Node<MaterialOp>, members: &[Node<MaterialOp>], dy: i32) -> i32 {
    let mut lo = -frame.y;
    let mut hi = WORLD - frame.bottom();
    for n in members {
        lo = lo.max(-n.y);
        hi = hi.min(MAX_NODE_Y - n.y);
    }
    dy.clamp(lo, hi.max(lo))
}

/// R899 / R1257 / R1596 — the value slot an authored-value edit writes.
///
/// Both arms are one thing now: a value authored on one of the node's own ports
/// ([`Node::values`], R1594). The editor stored them in two different fields —
/// `input_defaults[port]` and `output_const` — and R1594 recorded that those
/// were "one mechanism's two temporary fields".
#[derive(Clone, Copy)]
enum NodeValueTarget {
    /// Input port `n`'s pin default (`node.<id>.input_default.<n>`, R899).
    InputDefault(usize),
    /// A source node's output constant (`node.<id>.value`, R1257).
    OutputConst,
}

impl NodeValueTarget {
    /// Which port of the node this names.
    fn port(self) -> PortRef {
        match self {
            NodeValueTarget::InputDefault(port) => PortRef::input(uport(port)),
            NodeValueTarget::OutputConst => PortRef::output(0),
        }
    }

    /// The undo label for writing it.
    const fn label(self) -> &'static str {
        match self {
            NodeValueTarget::InputDefault(_) => "Set port default",
            NodeValueTarget::OutputConst => "Set source value",
        }
    }
}

/// R853 / R918 — clamp `(x, y)` into the world bounds and write node `id`'s
/// position, returning `(before, after)` window positions (`None` for an absent
/// id). The non-journaling reposition primitive shared by `set_node_pos`
/// (the capture drag, called once per frame) and [`apply_set_pos`] (the
/// journaling single-move funnel) — the ONE place a node's position is clamped
/// and written.
fn set_pos_clamped(
    document: &Rc<Signal<Graph>>,
    id: NodeId,
    x: i32,
    y: i32,
) -> Option<((i32, i32), (i32, i32))> {
    let mut result = None;
    document.set_with(|prev| {
        let mut next = prev.clone();
        if let Some(node) = next.tree_mut(TREE).and_then(|tree| tree.node_mut(id)) {
            let before = (node.x, node.y);
            node.x = clamp_node_x(x);
            node.y = clamp_node_y(y);
            result = Some((before, (node.x, node.y)));
        }
        next
    });
    result
}

/// R1183 — the graph-space point under a canvas **pixel** `(cx, cy)`: add the
/// pan offset (world px) then divide by zoom (`canvas = world − offset`,
/// `world = graph · zoom`). The single inverse-projection every canvas→graph
/// consumer routes through — [`cursor_graph_at`] (fraction form), the
/// coordinator's [`NodeGraphExternal::cursor_graph`] / `viewport.{x,y}` query,
/// the auto-pan tick, and the cursor-anchored zoom
/// ([`NodeGraphExternal::set_zoom_anchored`]) — so a basis change touches one
/// site. Its forward twin — the pan offset that places a graph point under a
/// canvas px — is [`graph_anchor_offset`] (R1191 closed this follow-up).
fn canvas_to_graph(scroll: &ScrollState, zoom: f64, cx: f64, cy: f64) -> (f64, f64) {
    let (ox, oy) = scroll.offset();
    ((f64::from(ox) + cx) / zoom, (f64::from(oy) + cy) / zoom)
}

/// R1191 — the forward twin of [`canvas_to_graph`]: the pan **offset** (world
/// px) that places graph point `(gx, gy)` under canvas px `(cx, cy)`. Solves the
/// same affine (`canvas = graph·zoom − offset`) for the offset instead of the
/// graph point, so the cursor-anchored zoom
/// ([`NodeGraphExternal::set_zoom_anchored`]), [`NodeGraphExternal::frame_all`],
/// and the `viewport.{x,y}` RPC pan write share ONE forward-projection site —
/// closing the asymmetry the R1183 [`canvas_to_graph`] rustdoc flagged as a
/// pre-existing follow-up. This is a DIFFERENT projection from [`wpx`] (the
/// rounded graph→world-px scaler used for node / edge / port paint positions AND
/// the world extent): `wpx` maps graph→world-px, this maps graph→pan-offset;
/// they share only the `·zoom` scaling, and `wpx` stays offset-free because the
/// scroll container applies the pan offset separately.
fn graph_anchor_offset(gx: f64, gy: f64, zoom: f64, cx: f64, cy: f64) -> (f64, f64) {
    (gx * zoom - cx, gy * zoom - cy)
}

/// R1220 — the canvas **pixel** a graph point `(gx, gy)` paints at: the exact
/// inverse of [`canvas_to_graph`] (`canvas = graph·zoom − offset`), so a
/// screen-space overlay OUTSIDE the world scroll (the pin-drop menu, placed like
/// the title / status chrome) can sit over the graph point it targets. Distinct
/// from [`graph_anchor_offset`] (which solves the same affine for the *pan
/// offset* an anchored zoom writes) and from [`wpx`] (the offset-free graph→world
/// scaler the scrolled node paint uses): this one folds the current pan offset in,
/// because the overlay is not inside the scroll that would apply it.
fn graph_to_canvas(scroll: &ScrollState, zoom: f64, gx: f64, gy: f64) -> (f64, f64) {
    let (ox, oy) = scroll.offset();
    (gx * zoom - f64::from(ox), gy * zoom - f64::from(oy))
}

/// R877 / R1182 — the graph-space point under a canvas cursor **fraction**
/// (the `pointer_move` / `wheel` / tick basis): un-normalise the fraction to
/// canvas px, then delegate to the [`canvas_to_graph`] inverse-projection SSOT.
fn cursor_graph_at(scroll: &ScrollState, zoom: f64, x_rel: f64, y_rel: f64) -> (f64, f64) {
    canvas_to_graph(
        scroll,
        zoom,
        x_rel * f64::from(WIN_W),
        y_rel * f64::from(WIN_H),
    )
}

/// R1182 — re-derive every dragged member's position from the cursor's
/// graph-space point `(gx, gy)` and its own grab anchor. The shared node-follow
/// kernel: `pointer_move`'s live preview and the auto-pan tick both call it so a
/// node stays pinned under the cursor whether the cursor moved or the viewport
/// scrolled beneath it. The caller owns the reactive [`batch`] (one cascade per
/// frame).
fn follow_members(
    document: &Rc<Signal<Graph>>,
    members: &[(NodeId, f64, f64, i32, i32)],
    gx: f64,
    gy: f64,
) {
    for &(id, grab_dx, grab_dy, _, _) in members {
        set_pos_clamped(
            document,
            id,
            round_i32(gx + grab_dx),
            round_i32(gy + grab_dy),
        );
    }
}

/// R1182 — the auto-pan push along one axis for a canvas cursor fraction
/// `frac` (0 = the low edge, 1 = the high edge). `0` in the dead centre,
/// ramping linearly to `-1` at/over the low rim and `+1` at/over the high rim
/// (a node dragged past the canvas edge under the capture lock — `frac`
/// outside `[0, 1]` — saturates at full push rather than reversing).
fn autopan_push(frac: f64) -> f64 {
    if frac < AUTOPAN_MARGIN {
        ((frac / AUTOPAN_MARGIN) - 1.0).max(-1.0)
    } else if frac > 1.0 - AUTOPAN_MARGIN {
        ((frac - (1.0 - AUTOPAN_MARGIN)) / AUTOPAN_MARGIN).min(1.0)
    } else {
        0.0
    }
}

/// R918 — commit node `id`'s position to a clamped `(x, y)` and journal it as
/// one *coalescable* `GraphEdit`, so a Details-panel `x` edit then `y` edit
/// (or an arrow-nudge burst) fold into a single undo step. An unchanged position
/// journals nothing (the [`apply_rename`] / [`apply_set_node_value`] no-op
/// discipline). The ONE position-commit funnel the panel's PosX/PosY inline
/// editor and the `intervene node.<id>.{x,y}` arm share, so a panel edit and an
/// RPC move are one undoable mutation path ([[setter-wire-returns-read-outcome]]).
fn apply_set_pos(handle: &GraphHandle, id: NodeId, x: i32, y: i32) -> bool {
    if handle.graph().tree(TREE).and_then(|t| t.node(id)).is_none() {
        return false;
    }
    handle.edit("Move node", Some(BTreeSet::from([id])), |graph, _| {
        if let Some(node) = graph.tree_mut(TREE).and_then(|tree| tree.node_mut(id)) {
            node.x = clamp_node_x(x);
            node.y = clamp_node_y(y);
        }
    });
    true
}

/// R878 / R1596 — rename node (or frame) `id` to `text`, journalled as one undo
/// step. `false` for a blank / whitespace-only title (the node keeps its name)
/// or an absent id — the value rejection both the inline commit and the
/// `intervene <thing>.<id>.title` arm report.
///
/// A frame is a node, so the pre-R1596 `RenameCmd<T>` generic over two
/// collections collapses to one function over one document.
fn apply_rename(handle: &GraphHandle, id: NodeId, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(node) = handle.graph().tree(TREE).and_then(|t| t.node(id)).cloned() else {
        return false;
    };
    // R1596 — a frame is a node, so the label is DERIVED from what was renamed
    // rather than passed in. The editor reached the same two words through a
    // `RenameCmd<T>` generic over its two separate collections.
    if node.title() == trimmed {
        // R878 — a rename to the name it already shows journals nothing. The
        // check is on the DISPLAYED name, because `label: None` and
        // `label: Some(the kind's own name)` show the same title and would
        // otherwise be a document change nobody made.
        return true;
    }
    let label = if node.is_frame() {
        "Rename frame"
    } else {
        "Rename node"
    };
    handle.edit(label, None, |graph, _| {
        if let Some(node) = graph.tree_mut(TREE).and_then(|tree| tree.node_mut(id)) {
            node.label = Some(trimmed.to_owned());
        }
    });
    true
}

/// R1257 / R1596 — write `value` onto one of node `id`'s ports (an unwired
/// input's default, or a source's output constant), journalled as one undo step.
///
/// R1594 made the two the SAME mechanism — [`Node::values`], keyed by a
/// [`PortRef`] — so the two commands the editor kept are one call whose only
/// difference is which port [`NodeValueTarget::port`] names. The crate refuses a
/// value whose type the port cannot hold, which is why this answers a `bool`
/// rather than assuming.
fn apply_set_node_value(
    handle: &GraphHandle,
    id: NodeId,
    target: NodeValueTarget,
    value: CellValue,
) -> bool {
    // R900 / R1596 — the no-op guard compares by TOTAL order, so re-authoring
    // the same value journals nothing even when that value is a NaN. The generic
    // funnel cannot make this call: it compares whole documents with `PartialEq`,
    // and `NaN != NaN` there, so a repeat NaN write would look like a change.
    // Float semantics belong to the value type, which is why the check is here
    // and `CellValue::value_eq` is what makes it.
    let carried = match target.port().side {
        Side::Input => input_default(&handle.graph(), id, uidx(target.port().index)),
        Side::Output => source_const(&handle.graph(), id),
    };
    if carried.is_some_and(|held| held.value_eq(&value)) {
        // Valid, and nothing to journal — which are two different answers the
        // caller does not have to tell apart: an `intervene` maps `false` to
        // `UnknownPath`, so a legitimate re-write must not report one.
        return true;
    }
    handle.edit(target.label(), None, |graph, _| {
        let _ = graph.set_port_value(TREE, id, target.port(), value);
    })
}

/// R901 — the [`CellKind`] of node `node`'s input port `port`'s default, or
/// `None` when the node / port is absent. Drives the inline editor's keystroke
/// gate (a `Float` accepts digits / sign / `.`, a `Color` hex digits + `#`)
/// and its commit parse — the one place a port default's editor type is read.
fn port_default_kind(graph: &Graph, node: NodeId, port: usize) -> Option<CellKind> {
    input_default(graph, node, port).map(|v| v.kind())
}

/// R1264 — the [`CellKind`] of node `node`'s authored output constant, or `None`
/// when the node is absent / not a source. The output-side twin of
/// [`port_default_kind`]: it drives the source-value editor's keystroke gate and
/// its commit parse (a `Float` source accepts digits / sign / `.`, a `Color`
/// source hex digits + `#`). Read straight off the stored [`CellValue`], not a
/// port lookup — the constant *is* the typed value.
fn source_const_kind(graph: &Graph, node: NodeId) -> Option<CellKind> {
    source_const(graph, node).map(|v| v.kind())
}

/// R901 — commit inline-editor `text` into `target` through the matching
/// field SSOT: a title routes to [`apply_rename`] (trim / reject-empty), a
/// port default parses by the port's [`CellKind`] and routes to
/// [`apply_set_node_value`] (a malformed numeric / hex keeps the prior value — no
/// data loss, the `CellKind::parse` contract). The ONE place an inline commit
/// dispatches by target, shared by the keyboard / blur [`commit_edit`] and the
/// begin-edit migration (committing a different in-flight target before
/// opening a new one), so the two can never drift.
fn apply_edit_commit(handle: &GraphHandle, target: EditTarget, text: &str) {
    match target {
        EditTarget::Title(id) => {
            let _ = apply_rename(handle, id, text);
        }
        EditTarget::PortDefault { node, port } => {
            if let Some(value) =
                port_default_kind(&handle.graph(), node, port).and_then(|k| k.parse(text))
            {
                let _ =
                    apply_set_node_value(handle, node, NodeValueTarget::InputDefault(port), value);
            }
        }
        // R918 — a position edit parses the typed coordinate and routes to the
        // shared `apply_set_pos` funnel. A malformed value keeps the prior
        // position (no data loss — the `CellKind::Int` keystroke gate already
        // bars non-numeric input, this guards a lone `-` or empty field).
        EditTarget::PosX(id) => {
            let graph = handle.graph();
            if let (Ok(coord), Some((node, _))) =
                (text.trim().parse::<i32>(), kind_node(&graph, id))
            {
                let _ = apply_set_pos(handle, id, coord, node.y);
            }
        }
        EditTarget::PosY(id) => {
            let graph = handle.graph();
            if let (Ok(coord), Some((node, _))) =
                (text.trim().parse::<i32>(), kind_node(&graph, id))
            {
                let _ = apply_set_pos(handle, id, node.x, coord);
            }
        }
        // R1264 — a source-const edit parses by the constant's own [`CellKind`]
        // and routes through the SAME [`apply_set_node_value`] /
        // [`NodeValueTarget::OutputConst`] funnel the AI-first `intervene
        // node.<id>.value` uses (R1257), so the card editor and the RPC write can
        // never drift. A malformed numeric / hex keeps the prior value — the
        // `CellKind::parse` no-data-loss contract.
        EditTarget::SourceConst(id) => {
            if let Some(value) = source_const_kind(&handle.graph(), id).and_then(|k| k.parse(text))
            {
                let _ = apply_set_node_value(handle, id, NodeValueTarget::OutputConst, value);
            }
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
        // R879 — drop the removed members; an emptied set collapses to
        // `None` through the construction funnel.
        Selection::Nodes(mut members) => {
            members.retain(|id| !removed_nodes.contains(id));
            Selection::from_nodes(members)
        }
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
    storage: Rc<AppStorage>,
    /// R877 — the canvas zoom (shared with the view fn's projection).
    zoom: Rc<Signal<f64>>,
    /// R877 — the canvas pan (the `ScrollAxis::Both` world scroll's state).
    scroll: Rc<ScrollState>,
    /// R878 / R901 — what the shared inline field is editing (shared with the
    /// view fn's title-or-field / pin-default switch and the keyboard / blur
    /// commit paths).
    editing: Rc<Signal<Option<ActiveEdit>>>,
    /// R878 — the shared inline field's text buffer
    /// ([`use_text_edit_state`]`(EDIT_TF_TAG)`) — `begin_edit` seeds it with
    /// the target's current text, a commit reads it back.
    edit_buffer: Rc<TextEditState>,
    /// R880 — the live marquee rect (shared with the view fn's rubber-band
    /// paint; [`use_marquee_rect`]).
    marquee_rect: Rc<Signal<Option<MarqueeRect>>>,
    /// R1182 — the node-drag latch, shared with the [`AutoPan`] driver
    /// ([`use_node_drag`]) so both read one authoritative drag state (the
    /// drag's rim-probe cursor rides inside it, [`NodeDragStart::cursor`]).
    node_drag: Rc<RefCell<Option<NodeDragStart>>>,
    /// R1220 — the open pin-drop create menu, shared with the view fn's
    /// floating-menu paint ([`use_pin_create`]).
    pin_create: Rc<Signal<Option<PinCreate>>>,
}

/// The node-graph coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) plus the internal drag latches.
struct NodeGraphExternal {
    /// R1596 — the graph: nodes, wires, frames, authored port values and the id
    /// counters, as one [`Document`]. Six holders before this round.
    document: Rc<Signal<Graph>>,
    /// The single selection (node | edge | none) — a sum type over stable ids,
    /// so node/edge selection is mutually exclusive AND survives an unrelated
    /// delete (a dangling selection is pruned to `None` by [`validate_after`]
    /// at record time, carried as the edit's `sel_after`).
    selection: Rc<Signal<Selection>>,
    preview: Rc<Signal<Option<Preview>>>,
    /// R851 — the shared undo history every structural mutation records onto
    /// (the same `Rc` the [`UndoStackExternal`] and the keyboard path reach).
    undo: Rc<UndoStack>,
    /// R852 — the platform graph store behind `save` / `load`.
    storage: Rc<AppStorage>,
    /// R877 — canvas zoom; the same Signal the view projects through.
    zoom: Rc<Signal<f64>>,
    /// R877 — canvas pan; the same `ScrollState` the world `ScrollNode`
    /// links (so the router's native wheel pan and the coordinator's
    /// anchored zoom write one offset).
    scroll: Rc<ScrollState>,
    /// R878 / R901 — the in-flight inline edit (`None` when idle); the same
    /// Signal the view fn's title-or-field / pin-default switch reads.
    editing: Rc<Signal<Option<ActiveEdit>>>,
    /// R878 — the shared inline field's text buffer (seeded on begin,
    /// read back on commit).
    edit_buffer: Rc<TextEditState>,
    /// Node grabbed for a capture-drag move (set on a node-body `PointerDown`).
    grabbed_node: Cell<Option<NodeId>>,
    /// R1182 — shared with the [`AutoPan`] driver (was a plain `RefCell`): the
    /// tick reads the same latch + members + rim-probe cursor
    /// ([`NodeDragStart::cursor`]) the coordinator writes.
    node_drag: Rc<RefCell<Option<NodeDragStart>>>,
    pending_press: Cell<PendingPress>,
    /// Edge under the most recent background press (the capture-seed
    /// `pointer_move` records it; a background `PointerUp` consumes it).
    pending_edge_hit: Cell<Option<EdgeId>>,
    /// R1243 — a reroute splice ARMED by a double-click on a wire, fired on the
    /// trailing in-place `PointerUp` (`Some(edge)` between the `DoubleClick` and
    /// its release). Deferred to the release — not run in the `DoubleClick` arm —
    /// so a double-click that becomes a DRAG (a marquee begun on a wire) marquees
    /// instead of splicing: the release routes to `apply_marquee`, and this arm
    /// is dropped by `reset_gesture`.
    pending_reroute_splice: Cell<Option<EdgeId>>,
    /// R880 — the in-flight background gesture (the marquee's press anchor +
    /// dead-zone latch; `None` while no background press is held).
    marquee: RefCell<Option<MarqueeStart>>,
    /// R880 — the shared marquee paint rect (the same Signal
    /// [`use_marquee_rect`] hands the view fn). `Some` exactly while the
    /// gesture's `live` latch is set.
    marquee_rect: Rc<Signal<Option<MarqueeRect>>>,
    /// R1220 — the open pin-drop create menu (`None` when closed); the same
    /// Signal [`use_pin_create`] hands the view fn's floating-menu paint.
    pin_create: Rc<Signal<Option<PinCreate>>>,
}

impl NodeGraphExternal {
    fn new(
        document: Rc<Signal<Graph>>,
        selection: Rc<Signal<Selection>>,
        preview: Rc<Signal<Option<Preview>>>,
        services: GraphServices,
    ) -> Self {
        Self {
            document,
            selection,
            preview,
            undo: services.undo,
            storage: services.storage,
            zoom: services.zoom,
            scroll: services.scroll,
            editing: services.editing,
            edit_buffer: services.edit_buffer,
            marquee_rect: services.marquee_rect,
            grabbed_node: Cell::new(None),
            node_drag: services.node_drag,
            pin_create: services.pin_create,
            pending_press: Cell::new(PendingPress::None),
            pending_edge_hit: Cell::new(None),
            pending_reroute_splice: Cell::new(None),
            marquee: RefCell::new(None),
        }
    }

    /// The document as it stands.
    fn graph(&self) -> Graph {
        self.document.get()
    }

    /// R1596 — the model as the tests read it: three derivations over the one
    /// document, where they used to read three separate reactive cells.
    #[cfg(test)]
    fn nodes(&self) -> Vec<Node<MaterialOp>> {
        kind_node_vec(&self.graph())
    }

    /// R1596 — what node `id`'s input port carries when nothing is wired to it:
    /// the value authored on THIS node's port, else the kind's own.
    #[cfg(test)]
    fn port_default(&self, id: NodeId, port: usize) -> Option<CellValue> {
        input_default(&self.graph(), id, port)
    }

    /// R1596 — the id the tree will mint next, which is what a gesture test
    /// needs to name the node a drop is ABOUT to create.
    #[cfg(test)]
    fn next_node_id(&self) -> u32 {
        let mut probe = self.graph();
        probe
            .add_node(TREE, NodeBody::Frame, 0, 0)
            .expect("the editor's tree")
            .0
    }

    /// R1596 — node `id`'s `(input, output)` port counts, off its signature.
    #[cfg(test)]
    fn arity(&self, id: NodeId) -> (usize, usize) {
        signature_of(&self.graph(), id).map_or((0, 0), |s| (s.inputs.len(), s.outputs.len()))
    }

    #[cfg(test)]
    fn edges(&self) -> Vec<Edge> {
        edges(&self.graph()).to_vec()
    }

    #[cfg(test)]
    fn frames(&self) -> Vec<Node<MaterialOp>> {
        frame_nodes(&self.graph()).cloned().collect()
    }

    fn node_count(&self) -> usize {
        kind_nodes(&self.graph()).count()
    }

    fn node_by_id(&self, id: NodeId) -> Option<Node<MaterialOp>> {
        kind_node(&self.graph(), id).map(|(node, _)| node.clone())
    }

    /// R1596 — the coordinator's three model holders as one [`GraphHandle`], so
    /// its mutations and the paint paths' run the same funnel.
    fn handle(&self) -> GraphHandle {
        GraphHandle {
            document: Rc::clone(&self.document),
            selection: Rc::clone(&self.selection),
            undo: Rc::clone(&self.undo),
        }
    }

    /// R1596 — the ONE structural mutation path ([`GraphHandle::edit`]).
    fn edit(
        &self,
        label: impl Into<Cow<'static, str>>,
        coalesce: Option<BTreeSet<NodeId>>,
        mutate: impl FnOnce(&mut Graph, &mut Selection),
    ) -> bool {
        self.handle().edit(label, coalesce, mutate)
    }

    /// R1245 — whether node `id` is a reroute knot (a wire-routing passthrough,
    /// [`NodeGeometry::is_reroute`]); a missing id reads `false`. Gates the
    /// double-click gesture: a knot dissolves, a compute node renames.
    fn is_reroute_node(&self, id: NodeId) -> bool {
        self.node_by_id(id).is_some_and(|n| n.is_reroute())
    }

    /// R916 — the absolute `node.<id>.<field>` path the Details panel's
    /// selection-relative `detail.<field>` resolves to, or `None` when the
    /// selection is not exactly one node. The single SSOT for the alias both the
    /// `detail` query (read) and the `detail` intervene (write) delegate through,
    /// so the panel's read and write address the identical node.
    fn selected_node_path(&self, field: &str) -> Option<String> {
        self.selection
            .get()
            .node()
            .map(|id| format!("node.{}.{field}", id.0))
    }

    // ── R877 viewport (pan = scroll offset, zoom = shared Signal) ──

    /// The graph-space point under a canvas-relative cursor fraction
    /// (the `pointer_move` / `wheel` coordinate basis): delegates to the
    /// [`cursor_graph_at`] projection SSOT the auto-pan tick also uses.
    fn cursor_graph(&self, x_rel: f64, y_rel: f64) -> (f64, f64) {
        cursor_graph_at(&self.scroll, self.zoom.get(), x_rel, y_rel)
    }

    /// Set the zoom, keeping the graph point under the canvas-px anchor
    /// `(sx, sy)` pinned (the cursor-anchored wheel zoom; the keyboard /
    /// RPC paths anchor at the canvas centre). The world extent scales
    /// with the zoom, so the scroll maxima are rewritten in the same
    /// reactive batch as the offset — one atomic viewport update
    /// ([[signal-batch-atomic-multi-axis-update]]); the next layout pass
    /// re-derives the identical maxima from the declared world size.
    /// Returns whether the zoom actually changed (already clamped =
    /// no-op).
    fn set_zoom_anchored(&self, target: f64, sx: f64, sy: f64) -> bool {
        let old = self.zoom.get();
        let zoom = target.clamp(ZOOM_MIN, ZOOM_MAX);
        if (zoom - old).abs() < f64::EPSILON {
            return false;
        }
        // R1183 — the graph point under the anchor px, via the inverse SSOT
        // (was an inline copy of `canvas_to_graph`'s affine). R1191 — the offset
        // that re-pins it under the anchor at the new zoom, via the forward SSOT.
        let (gx, gy) = canvas_to_graph(&self.scroll, old, sx, sy);
        let (ox, oy) = graph_anchor_offset(gx, gy, zoom, sx, sy);
        self.apply_viewport(zoom, ox, oy);
        true
    }

    /// The single viewport writer: zoom + world maxima + pan offset land in
    /// ONE reactive batch ([[signal-batch-atomic-multi-axis-update]]), so no
    /// paint can observe a zoom whose maxima (or offset) belong to the old
    /// scale. The maxima formula (`WORLD·zoom − canvas`) has this one home;
    /// the layout pass re-derives the identical bounds from the declared
    /// world extent on the next frame.
    fn apply_viewport(&self, zoom: f64, ox: f64, oy: f64) {
        batch(|| {
            self.zoom.set(zoom);
            let content = wpx(WORLD, zoom);
            self.scroll.set_max(
                content - i32::try_from(WIN_W).unwrap_or(0),
                content - i32::try_from(WIN_H).unwrap_or(0),
            );
            self.scroll.scroll_to(round_i32(ox), round_i32(oy));
        });
    }

    /// Centre-anchored zoom step (keyboard `Ctrl`+`=` / `Ctrl`+`-` /
    /// `Ctrl`+`0`, and the `intervene viewport.zoom` RPC write).
    fn set_zoom_centered(&self, target: f64) -> bool {
        self.set_zoom_anchored(target, f64::from(WIN_W) / 2.0, f64::from(WIN_H) / 2.0)
    }

    /// R877 — `frame_all` (`f`, the engine / the DCC "frame" idiom): fit
    /// the node bounding box into the canvas with [`FRAME_MARGIN`],
    /// clamped to the zoom range, and centre it. `false` on an empty
    /// graph (nothing to frame, viewport unchanged).
    fn frame_all(&self) -> bool {
        let graph = self.graph();
        // R948 — the union bbox over all nodes (the [`node_bounds`] SSOT, also
        // the selection-bbox source for `align_selected`).
        let Some((min_x, min_y, max_x, max_y)) = node_bounds(kind_nodes(&graph).map(|(n, _)| n))
        else {
            return false;
        };
        let bw = f64::from((max_x - min_x).max(1));
        let bh = f64::from((max_y - min_y).max(1));
        let fit_w = f64::from(i32::try_from(WIN_W).unwrap_or(0) - 2 * FRAME_MARGIN) / bw;
        let fit_h = f64::from(i32::try_from(WIN_H).unwrap_or(0) - 2 * FRAME_MARGIN) / bh;
        let zoom = fit_w.min(fit_h).clamp(ZOOM_MIN, ZOOM_MAX);
        // The bbox centre in GRAPH space (a tuple, so it reads distinctly from
        // the forward projection's canvas-px anchor args below).
        let centre = (
            f64::from(min_x + max_x) / 2.0,
            f64::from(min_y + max_y) / 2.0,
        );
        // R1191 — the offset that pins the bbox graph centre at the canvas
        // centre, via the forward-projection SSOT.
        let (ox, oy) = graph_anchor_offset(
            centre.0,
            centre.1,
            zoom,
            f64::from(WIN_W) / 2.0,
            f64::from(WIN_H) / 2.0,
        );
        self.apply_viewport(zoom, ox, oy);
        true
    }

    /// R853/R879/R1596 — journal an already-applied (multi-)node move as ONE
    /// undo step.
    ///
    /// The live drag / nudge / intervene paths write positions straight onto the
    /// document (one write per frame), so the `after` state is the document as
    /// it stands and the `before` is reconstructed by putting each moved node
    /// back — exact, because a move changes nothing else about the document.
    /// The stack's coalescing then folds a contiguous `coalescable` same-member
    /// run (a nudge burst) into one step. Unmoved members are dropped; an
    /// all-unmoved set journals nothing.
    fn record_moves(&self, mut moves: Vec<NodeMove>, coalescable: bool) {
        moves.retain(|(_, before, after)| before != after);
        if moves.is_empty() {
            return;
        }
        let after = self.graph();
        let mut before = after.clone();
        for &(id, was, _) in &moves {
            if let Some(node) = before.tree_mut(TREE).and_then(|t| t.node_mut(id)) {
                (node.x, node.y) = was;
            }
        }
        let label = if moves.len() == 1 {
            Cow::Borrowed("Move node")
        } else {
            Cow::Owned(format!("Move {} nodes", moves.len()))
        };
        let members: BTreeSet<NodeId> = moves.iter().map(|&(id, _, _)| id).collect();
        let selection = self.selection.get();
        self.undo.push_applied(GraphEdit {
            document: Rc::clone(&self.document),
            selection: Rc::clone(&self.selection),
            label,
            before,
            after,
            sel_before: selection.clone(),
            sel_after: selection,
            coalesce: coalescable.then_some(members),
        });
    }

    /// R852 — snapshot the persistable graph (nodes + edges + the monotonic id
    /// counters; the selection is transient and omitted).
    fn snapshot(&self) -> SerializedGraph {
        SerializedGraph {
            schema_version: PERSISTED_SCHEMA_VERSION,
            graph: self.graph(),
        }
    }

    /// R852 — the graph as a JSON string (the AI-first `query serialized` read).
    fn serialized_json(&self) -> String {
        serde_json::to_string(&self.snapshot()).unwrap_or_default()
    }

    /// R852 — replace the whole graph from a snapshot: swap nodes / edges,
    /// resume the id counters where the snapshot left off, drop the selection
    /// / preview, and clear the undo history — the opened document is a fresh
    /// baseline (the undo stack "open clears the stack" model). The single
    /// restore path behind `set_graph` / `load`, so every entry point clears undo
    /// identically.
    fn apply_snapshot(&self, g: SerializedGraph) {
        self.document.set(g.graph);
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
    /// JSON or a schema-version mismatch (`false`, the graph unchanged). R1258 /
    /// R1259 — also rejects a **structurally-invalid** blob. So an untrusted
    /// `set_graph` blob (§2 #2 — RPC is the AI-first write path) fails LOUD
    /// (graph unchanged, the AI sees `false`) rather than silently evaluating an
    /// ill-typed / malformed graph to the wrong value or a permanent `null`.
    ///
    /// R1596 — the check is [`Document::validate`], and it is **wider than the
    /// four the editor listed**: a dangling link, an over-fed input, a wire whose
    /// types cannot cross, a cycle, a node claiming a parent that is not there or
    /// is not a frame, a containment cycle, and (for the group axis this editor
    /// does not yet use) a dangling instance and a recursive definition. The
    /// editor hand-maintained three id counters and gated a load on each leading
    /// its own ids; a tree mints from its own counter, so that whole class of
    /// blob is unrepresentable rather than rejected.
    fn load_json(&self, json: &str) -> bool {
        let Ok(g) = serde_json::from_str::<SerializedGraph>(json) else {
            return false;
        };
        if g.schema_version != PERSISTED_SCHEMA_VERSION {
            return false;
        }
        if !graph_invariants_hold(&g.graph) {
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
    /// record the `MoveNodeCmd` once the gesture / keystroke settles.
    fn set_node_pos(&self, id: NodeId, x: i32, y: i32) -> bool {
        set_pos_clamped(&self.document, id, x, y).is_some()
    }

    /// R849 — create a new node of [`PALETTE`] kind `kind` at the next cascade
    /// position, minting a fresh stable [`NodeId`] (monotonic, never reused),
    /// and select it. Returns the new id, or `None` for an out-of-range kind.
    /// The single mutation behind both a palette card click (`handle_send`)
    /// and the `add_node` RPC verb — the graph can finally *grow*, not only be
    /// rearranged. A new node has no edges, so no edge / selection bookkeeping
    /// is needed (the stable-id model: adding is purely additive).
    fn add_node(&self, kind: usize) -> Option<NodeId> {
        // Cascade in minted order from the spawn point so repeated adds fan out
        // instead of stacking exactly on one another. R877 — the spawn point is
        // a fixed *canvas* position projected into graph space through the
        // current viewport, so a new node always lands in the visible view
        // (spawning at a fixed graph point would drop it off-screen once the
        // canvas has panned away). R1411 — the step reads the id ABOUT to be
        // minted (`next_node_id` peek == the value `add_node_at` will assign), so
        // the fan-out is bit-identical to before the `add_node_at` extraction.
        // R1596 — the cascade counts the nodes that exist rather than peeking at
        // a counter, which is the same number for a graph that has only grown
        // and an honest one for a graph nodes have been deleted from (the
        // counter kept climbing there, so the fan-out drifted off the point).
        let step = i32::try_from(self.node_count()).unwrap_or(0) % 8;
        let (gx, gy) = self.cursor_graph(
            f64::from(SPAWN_X) / f64::from(WIN_W),
            f64::from(SPAWN_Y) / f64::from(WIN_H),
        );
        let x = clamp_node_x(round_i32(gx) + step * SPAWN_STEP);
        let y = clamp_node_y(round_i32(gy) + step * SPAWN_STEP);
        self.add_node_at(kind, x, y)
    }

    /// R1411 — instantiate palette `kind` at graph point `(x, y)` (already
    /// clamped) in one undo step, selecting the new node. The position SSOT the
    /// two creation gestures share: [`add_node`](Self::add_node) (a click / RPC
    /// add at the spawn point + fan-out cascade) and the drag-to-instantiate drop
    /// ([`drag_release`](Self::drag_release), the drop point). Returns the new
    /// node id, or `None` for an out-of-range `kind`.
    fn add_node_at(&self, kind: usize, x: i32, y: i32) -> Option<NodeId> {
        let &(title, ..) = PALETTE.get(kind)?;
        let mut made = None;
        // `record` applies the edit forward — adding the node and selecting it —
        // so a single Ctrl+Z removes it again.
        self.edit(format!("Add {title}"), None, |graph, sel| {
            if let Some(id) = add_palette_node(graph, kind, x, y) {
                *sel = Selection::single(id);
                made = Some(id);
            }
        });
        made
    }

    // ── R1220 pin-drop create menu (drag off a pin → typed menu → auto-wire) ──

    /// The source output's [`PortType`] + the current candidate [`PALETTE`]
    /// indices for the open menu `pc` (its `filter` applied). `None` if the
    /// source pin has gone invalid (an undo removed its node while the menu was
    /// open) — every menu path re-derives from here, so a mid-menu delete never
    /// commits against a stale pin.
    fn pin_candidates(&self, pc: &PinCreate) -> Option<(PortType, Vec<usize>)> {
        let from_ty = output_type(&self.graph(), pc.from_node, pc.from_port)?;
        Some((from_ty, pin_create_candidates(from_ty, &pc.filter)))
    }

    /// Open the pin-drop create menu for output `(from_node, from_port)`, placing
    /// the created node at `at_graph` and the floating menu at canvas px
    /// `at_screen`. `false` (no menu) when the pin is invalid OR nothing can
    /// consume its type — preserving the pre-R1220 "release in empty space
    /// cancels" behaviour for an output with no compatible target kind. The one
    /// entry point behind the live [`drag_release`](Self::drag_release) empty-canvas
    /// branch and the `open_pin_create` RPC verb.
    fn open_pin_create(
        &self,
        from_node: NodeId,
        from_port: usize,
        at_graph: (i32, i32),
        at_screen: (u32, u32),
    ) -> bool {
        let Some(from_ty) = output_type(&self.graph(), from_node, from_port) else {
            return false;
        };
        if pin_create_candidates(from_ty, "").is_empty() {
            return false;
        }
        self.pin_create.set(Some(PinCreate {
            from_node,
            from_port,
            at_graph: (clamp_node_x(at_graph.0), clamp_node_y(at_graph.1)),
            at_screen,
            filter: String::new(),
            highlight: 0,
        }));
        true
    }

    /// Set the type-to-narrow filter text, clamping the roving highlight into the
    /// re-filtered list. `false` when no menu is open.
    fn set_pin_filter(&self, text: &str) -> bool {
        let Some(mut pc) = self.pin_create.get() else {
            return false;
        };
        text.clone_into(&mut pc.filter);
        let n = self.pin_candidates(&pc).map_or(0, |(_, c)| c.len());
        pc.highlight = pc.highlight.min(n.saturating_sub(1));
        self.pin_create.set(Some(pc));
        true
    }

    /// Move the roving highlight by `delta` (wrapping — arrow past an end returns
    /// to the other, the menu-roving convention). `false` when no menu is open or
    /// the filtered list is empty.
    fn move_pin_highlight(&self, delta: i32) -> bool {
        let Some(mut pc) = self.pin_create.get() else {
            return false;
        };
        let Some((_, cands)) = self.pin_candidates(&pc) else {
            return false;
        };
        let Ok(len) = i32::try_from(cands.len()) else {
            return false;
        };
        if len == 0 {
            return false;
        }
        let cur = i32::try_from(pc.highlight).unwrap_or(0);
        pc.highlight = usize::try_from((cur + delta).rem_euclid(len)).unwrap_or(0);
        self.pin_create.set(Some(pc));
        true
    }

    /// Commit [`PALETTE`] kind `kind`: mint the node at the drop point AND the
    /// auto-wire edge `(from_node, from_port) → (new node, first compatible
    /// input)`, recorded as ONE [`GraphEdit`] so a single Ctrl+Z removes both
    /// (create-and-wire is atomic — the whole point of the gesture). Only a
    /// *current candidate* commits: a stale / ill-typed kind is rejected, leaving
    /// the menu open (the RPC gate — the GUI only ever sends candidates). Returns
    /// the new node's id, selects it, and closes the menu.
    fn commit_pin_create_kind(&self, kind: usize) -> Option<NodeId> {
        let pc = self.pin_create.get()?;
        let (from_ty, cands) = self.pin_candidates(&pc)?;
        if !cands.contains(&kind) {
            return None;
        }
        let target_port = first_compatible_input(kind, from_ty)?;
        let &(title, ..) = PALETTE.get(kind)?;
        let mut made = None;
        self.edit(format!("Add {title} + wire"), None, |graph, sel| {
            let Some(id) = add_palette_node(graph, kind, pc.at_graph.0, pc.at_graph.1) else {
                return;
            };
            // The wire is type-checked by the same gate a hand-drawn one is, so
            // a candidate the menu offered and a wire the canvas accepts cannot
            // disagree — `first_compatible_input` picked the port off the very
            // relation `connect` will apply.
            if graph
                .connect(
                    TREE,
                    Socket::new(pc.from_node, uport(pc.from_port)),
                    Socket::new(id, uport(target_port)),
                )
                .is_ok()
            {
                *sel = Selection::single(id);
                made = Some(id);
            }
        });
        if made.is_some() {
            self.pin_create.set(None);
        }
        made
    }

    /// Commit the highlighted candidate (the Enter / click-the-focused-item
    /// path). `None` (menu unchanged) when no menu is open or the filter left the
    /// list empty.
    fn commit_pin_create_highlighted(&self) -> Option<NodeId> {
        let pc = self.pin_create.get()?;
        let (_, cands) = self.pin_candidates(&pc)?;
        let kind = *cands.get(pc.highlight)?;
        self.commit_pin_create_kind(kind)
    }

    /// Close the menu without creating anything (Escape / click-away / the
    /// `cancel_pin_create` verb). `false` when no menu was open.
    fn cancel_pin_create(&self) -> bool {
        if self.pin_create.get().is_some() {
            self.pin_create.set(None);
            true
        } else {
            false
        }
    }

    /// R1220 — the open menu as an AI-first JSON snapshot (the `query pin_create`
    /// read; `Null` when closed): `{ from_node, from_port, at:{x,y} (graph units
    /// where the node lands), filter, candidates:[kind names in menu order],
    /// highlight }`. The introspection twin of the painted floating menu — same
    /// candidate list, same highlight — so an AI reads exactly what a user sees.
    fn pin_create_introspect(&self) -> IntrospectValue {
        let Some(pc) = self.pin_create.get() else {
            return IntrospectValue::Null;
        };
        // R1223 — gate on source validity, mirroring the paint + a11y menu
        // (which render nothing when the source pin is stale). Without this, a
        // source node deleted while the menu was open (e.g. `open_pin_create` →
        // `delete_node`) left `query pin_create` reporting an OPEN menu the user
        // could not see — an introspection-twin (§2 #2) violation — and, because
        // the modal keyboard keys off this query returning `Json`, trapped the
        // shell keyboard with no visible menu. `pin_candidates` is `None` exactly
        // when the source node / port is gone, so a stale menu now reads `Null`
        // (effectively closed) through the SAME gate paint and a11y apply.
        let Some((_, cands)) = self.pin_candidates(&pc) else {
            return IntrospectValue::Null;
        };
        let candidates: Vec<&str> = cands.into_iter().map(|k| PALETTE[k].0).collect();
        IntrospectValue::Json(serde_json::json!({
            "from_node": pc.from_node.0,
            "from_port": pc.from_port,
            "at": { "x": pc.at_graph.0, "y": pc.at_graph.1 },
            "filter": pc.filter,
            "candidates": candidates,
            "highlight": pc.highlight,
        }))
    }

    /// Add an edge output `(from_node, from_port)` → input `(to_node,
    /// to_port)`. Rejects a self-loop, a missing node, an out-of-range port, or
    /// a duplicate; an input port takes a single wire, so an existing
    /// connection into the target input is replaced (the canonical node-editor
    /// rule). The new edge mints a fresh stable [`EdgeId`].
    /// R929 — validate a prospective wire `(from_node,from_port) ->
    /// (to_node,to_port)` and, on success, return the existing edges it would
    /// **displace** (the single-wire-into-one-input rule). `None` rejects:
    /// self-loop / out-of-range or mistyped port / an exact duplicate wire.
    /// `ignore` excludes one edge id from the duplicate + displacement scans —
    /// the edge being reconnected, so re-dropping it on its own input never
    /// reads as a self-duplicate or self-displacement. The SSOT both
    /// [`add_edge`](Self::add_edge) and [`reconnect_edge`](Self::reconnect_edge)
    /// validate through, so a wire valid one way is valid the other (a
    /// divergence here would be a bug).
    fn validate_connection(
        &self,
        from_node: NodeId,
        from_port: usize,
        to_node: NodeId,
        to_port: usize,
        ignore: Option<EdgeId>,
    ) -> Option<Vec<Edge>> {
        if from_node == to_node {
            return None;
        }
        let graph = self.graph();
        let from = Socket::new(from_node, uport(from_port));
        let to = Socket::new(to_node, uport(to_port));
        // R1596 — the type gate is the crate's, asked WITHOUT committing:
        // `Document::conversion` answers what would happen to a value crossing
        // these two sockets, which is `None` when either port is missing (the
        // pre-R898 arity reject) and `Refused` when no value may cross. The
        // editor used to spell this as a predicate of its own beside a coercion
        // of its own, which is the pair R1593 made one declaration.
        if graph.conversion(TREE, from, to)?.is_refused() {
            return None;
        }
        let dup = edges(&graph)
            .iter()
            .any(|e| Some(e.id) != ignore && e.from == from && e.to == to);
        if dup {
            return None;
        }
        // Input single-wire rule: the new wire displaces any existing wire into
        // the same target input — reported so undo restores it.
        Some(
            edges(&graph)
                .iter()
                .copied()
                .filter(|e| Some(e.id) != ignore && e.to == to)
                .collect(),
        )
    }

    /// R930.1 — mint a fresh edge `(from -> to)` and record it as one
    /// [`GraphEdit`] that removes `removed` and adds the new wire, pruning an
    /// edge selection the removal strands. The shared commit tail of
    /// [`add_edge`](Self::add_edge) and [`reconnect_edge`](Self::reconnect_edge):
    /// both must build the identical `GraphDelta` shape + `sel_after` prune (a
    /// divergence would split undo / selection between the two paths), so it is
    /// one SSOT. Each caller's `validate_connection` gate and its `removed` set
    /// are the only differences (R929 factored the validation but left this
    /// commit tail duplicated — the missing half of that lift).
    // R1596 — the two monotonic id `Cell`s are gone: `Document::add_node` and
    // `Document::connect` mint their own, so "a deleted id is never handed out
    // again" is a property of the tree rather than of two counters a save / load
    // path had to carry and re-seed in step.
    fn commit_new_edge(
        &self,
        label: &'static str,
        from_node: NodeId,
        from_port: usize,
        to_node: NodeId,
        to_port: usize,
        removed: &[Edge],
    ) -> bool {
        let removed_ids: Vec<EdgeId> = removed.iter().map(|e| e.id).collect();
        // R1596 — the CRATE decides whether the wire may exist, and this reports
        // what it decided. `connect` refuses a type mismatch, a second producer
        // on one input and a wire that would close a CYCLE, and the last of
        // those the editor's own gate never had; swallowing the refusal here
        // would answer `true` for a wire that was never made — which a caller
        // maps straight onto the `add_edge` verb's wire contract.
        let mut wired = false;
        self.edit(label, None, |graph, sel| {
            for id in &removed_ids {
                let _ = graph.disconnect(TREE, *id);
            }
            wired = graph
                .connect(
                    TREE,
                    Socket::new(from_node, uport(from_port)),
                    Socket::new(to_node, uport(to_port)),
                )
                .is_ok();
            if wired {
                *sel = validate_after(sel.clone(), &[], &removed_ids);
            }
        });
        wired
    }

    fn add_edge(
        &self,
        from_node: NodeId,
        from_port: usize,
        to_node: NodeId,
        to_port: usize,
    ) -> bool {
        let Some(replaced) = self.validate_connection(from_node, from_port, to_node, to_port, None)
        else {
            return false;
        };
        self.commit_new_edge("Connect", from_node, from_port, to_node, to_port, &replaced)
    }

    /// R929 — move an existing edge's **target** to a new input port, keeping
    /// its source output: the canonical node-editor "reconnect" (grab a wired
    /// input and drop it elsewhere). One atomic [`GraphEdit`] — the old edge
    /// is removed and a fresh edge `(same source -> new input)` added in a
    /// single undo step (so one `Ctrl+Z` restores the original wiring), plus
    /// any wire it displaces at the new input (the single-wire rule, via
    /// [`validate_connection`](Self::validate_connection)). Re-dropping on the
    /// same input is a no-op `true`; an invalid target (self-loop / mistyped /
    /// missing port / exact duplicate) is a no-op `false`, leaving the edge as
    /// it was. The reconnected wire mints a fresh [`EdgeId`] — a reconnect is a
    /// new connection, consistent with the remove+add `GraphEdit` model.
    fn reconnect_edge(&self, edge_id: EdgeId, new_to_node: NodeId, new_to_port: usize) -> bool {
        let Some(old) = edge(&self.graph(), edge_id).copied() else {
            return false;
        };
        if old.to == Socket::new(new_to_node, uport(new_to_port)) {
            return true; // dropped back on its own input — nothing changed.
        }
        let Some(replaced) = self.validate_connection(
            old.from.node,
            uidx(old.from.port),
            new_to_node,
            new_to_port,
            Some(edge_id),
        ) else {
            return false;
        };
        let mut removed = vec![old];
        removed.extend(replaced);
        self.commit_new_edge(
            "Reconnect",
            old.from.node,
            uidx(old.from.port),
            new_to_node,
            new_to_port,
            &removed,
        )
    }

    /// R1235 — splice a **reroute node** into edge `edge_id`: a typed 1-in /
    /// 1-out passthrough dropped at the wire's midpoint so the connection routes
    /// `A -> R -> B` instead of `A -> B` (the Blueprint / material-editor
    /// "reroute" knot — bend a wire around for readability, then drag `R` to
    /// route it). The reroute adopts the wire's [`PortType`] on BOTH ports, so
    /// `A -> R` and `R -> B` are assignable exactly when `A -> B` was — the
    /// splice never weakens type-safety. ONE undoable [`GraphEdit`]: the original
    /// edge is removed and the node + its two edges are added together, so a
    /// single `Ctrl`+`Z` undoes the whole reroute. `None` for an unknown edge id.
    fn add_reroute(&self, edge_id: EdgeId) -> Option<NodeId> {
        let graph = self.graph();
        let wire = *edge(&graph, edge_id)?;
        // The wire's type is its SOURCE output's type; both reroute ports take
        // it, which is what makes the knot a `MaterialOp::Reroute(ty)`.
        let ty = output_type(&graph, wire.from.node, wire.from.port as usize)?;
        // Centre the reroute on the wire's straight midpoint (graph units — the
        // same space `edge_endpoints` + node positions live in).
        let (from, to) = edge_endpoints(&graph, &wire)?;
        let knot = MaterialOp::Reroute(ty);
        let mid_x = i32::midpoint(from.0, to.0) - KNOT_SIZE / 2;
        let mid_y = i32::midpoint(from.1, to.1) - KNOT_SIZE / 2;
        let mut made = None;
        self.edit("Insert reroute", None, |graph, sel| {
            let Ok(id) = graph.add_node(
                TREE,
                NodeBody::Kind(knot),
                clamp_node_x(mid_x),
                clamp_node_y(mid_y),
            ) else {
                return;
            };
            // The original wire goes first: its target input takes one link, and
            // `R -> B` is that link now.
            let _ = graph.disconnect(TREE, edge_id);
            let a_to_r = graph.connect(TREE, wire.from, Socket::new(id, 0));
            let r_to_b = graph.connect(TREE, Socket::new(id, 0), wire.to);
            if a_to_r.is_ok() && r_to_b.is_ok() {
                *sel = Selection::single(id);
                made = Some(id);
            }
        });
        made
    }

    /// R1235 — the `add_reroute` verb arm: splice a reroute into edge `<int>`,
    /// returning the new node id (`Null` for an unknown edge; a non-`Int` arg is
    /// a `TypeMismatch`).
    fn invoke_add_reroute(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let &IntrospectValue::Int(i) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
        Ok(match self.add_reroute(EdgeId(id)) {
            Some(n) => IntrospectValue::Int(i64::from(n.0)),
            None => IntrospectValue::Null,
        })
    }

    /// R1236 — the `dissolve_node` verb arm: dissolve node `<int>` (delete +
    /// reconnect), returning the `Bool` outcome (a non-`Int` arg is a
    /// `TypeMismatch`). Extracted to keep `invoke` within the line ceiling.
    fn invoke_dissolve_node(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let &IntrospectValue::Int(i) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
        Ok(IntrospectValue::Bool(self.dissolve_node(NodeId(id))))
    }

    /// R1264 — the `begin_edit_value` verb arm: open the inline editor on a
    /// source node's output constant (the AI-first / test twin of double-clicking
    /// the source-value label; the output-side peer of `begin_edit_default`). Arg
    /// = the node `<int>`, or `Null` for the single selected node (mirroring
    /// `begin_rename`). `false` on an unknown / non-source node (graph unchanged);
    /// a non-`Int`/`Null` arg is a `TypeMismatch`. Extracted to keep `invoke`
    /// within the line ceiling.
    fn invoke_begin_edit_value(
        &self,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let id = match args {
            &IntrospectValue::Int(i) => {
                NodeId(u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?)
            }
            IntrospectValue::Null => {
                let Some(id) = self.selection.get().node() else {
                    return Ok(IntrospectValue::Bool(false));
                };
                id
            }
            _ => return Err(InvokeError::TypeMismatch),
        };
        Ok(IntrospectValue::Bool(self.begin_edit_source_value(id)))
    }

    /// R899 — the `intervene node.<id>.input_default.<port>` write path. The
    /// value is type-checked against the port's kind by
    /// [`CellValue::with_intervene`] (a `Float` takes a float, a `Vector`/`Color`
    /// a `#RRGGBB[AA]` hex), then routed through the `apply_set_node_value` SSOT,
    /// so the AI write journals the same undoable `GraphEdit` an inline
    /// editor would. An unknown field / out-of-range port is an `UnknownPath`.
    fn intervene_input_default(
        &mut self,
        id: NodeId,
        field: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        let Some(port) = field
            .strip_prefix("input_default.")
            .and_then(|p| p.parse::<usize>().ok())
        else {
            return Err(InterveneError::UnknownPath);
        };
        let Some(current) = input_default(&self.graph(), id, port) else {
            return Err(InterveneError::UnknownPath);
        };
        let next = current.with_intervene(value)?;
        // R900 — gate the result like the `apply_rename` caller does (symmetry):
        // the port existence is pre-checked above so `false` is currently
        // unreachable, but threading the funnel's success keeps the contract
        // explicit and total rather than silently swallowing a future failure.
        if apply_set_node_value(
            &self.handle(),
            id,
            NodeValueTarget::InputDefault(port),
            next,
        ) {
            Ok(())
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    /// R1257 — the `intervene node.<id>.value` write path: author a **source**
    /// node's output constant (the output-side twin of
    /// [`Self::intervene_input_default`]). Gated on [`is_source`] —
    /// a compute op / sink / reroute has a *derived* value, so the write is
    /// `ReadOnly`. The value is typed against the source's current constant by
    /// [`CellValue::with_intervene`] (matching output port 0's kind) and routed
    /// through the same `apply_set_node_value` SSOT, so it journals an undoable
    /// `GraphEdit` just like a pin-default edit.
    fn intervene_source_value(
        &mut self,
        id: NodeId,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        let Some(current) = source_const(&self.graph(), id) else {
            return Err(InterveneError::ReadOnly);
        };
        let next = current.with_intervene(value)?;
        if apply_set_node_value(&self.handle(), id, NodeValueTarget::OutputConst, next) {
            Ok(())
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    /// Remove the edge with stable id `id` (no-op + `false` if absent). R851 —
    /// the edge is stored as a removed delta so undo re-inserts it verbatim.
    fn remove_edge(&self, id: EdgeId) -> bool {
        if edge(&self.graph(), id).is_none() {
            return false;
        }
        self.edit("Disconnect", None, |graph, sel| {
            if graph.disconnect(TREE, id).is_ok() {
                *sel = validate_after(sel.clone(), &[], &[id]);
            }
        });
        true
    }

    /// R1226 — the wire KNIFE: delete every edge the straight cut segment
    /// `a`-`b` (graph units) crosses, as ONE undoable [`GraphEdit`] (a single
    /// `Ctrl+Z` restores all cut wires with their stable ids — the multi-edge
    /// analogue of [`remove_edge`](Self::remove_edge)). Each edge is resolved to
    /// its port-center endpoints (the [`hit_test_edge`](Self::hit_test_edge)
    /// SSOT) and tested with [`edge_crosses_segment`]. Returns the cut edge ids
    /// (empty when the stroke crosses nothing — no undo entry recorded). This is
    /// the §2 AI-first primary path for a knife gesture: the live drag is a
    /// held mid-gesture the atomic `scene/drag` cannot snapshot (the R1114
    /// §2 #2 note), so the cut is expressed verb-first and the human gesture
    /// (a future canvas stroke) funnels through this same method.
    fn cut_wires(&self, a: (i32, i32), b: (i32, i32)) -> Vec<EdgeId> {
        let graph = self.graph();
        let ids: Vec<EdgeId> = edges(&graph)
            .iter()
            .filter(|e| {
                edge_endpoints(&graph, e)
                    .is_some_and(|(from, to)| edge_crosses_segment(from, to, a, b))
            })
            .map(|e| e.id)
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        self.edit("Cut wires", None, |graph, sel| {
            for &id in &ids {
                let _ = graph.disconnect(TREE, id);
            }
            *sel = validate_after(sel.clone(), &[], &ids);
        });
        ids
    }

    /// R1226 — the `cut_wires` invoke arm, extracted so the `invoke` match stays
    /// within the workspace `too_many_lines` ceiling. Parses `"x1,y1,x2,y2"` and
    /// returns the CSV of cut edge ids; a malformed spec Rejects, a non-string
    /// arg is a TypeMismatch.
    /// R1564 §5.15 (PINION-PR82) — "no node kind by that name", listing the
    /// palette. Two sites reach for it, and the palette is short and fixed, so
    /// a refusal that prints it turns a typo into a one-look fix.
    fn unknown_kind_reason(action: &str, name: &str) -> InvokeError {
        let palette: Vec<&str> = PALETTE.iter().map(|&(n, _)| n).collect();
        InvokeError::rejected(format!(
            "{action}: {name:?} is not a node kind (the palette offers: {})",
            palette.join(", ")
        ))
    }

    /// R1564 — the `begin_edit_default` arm's body, lifted for the same reason
    /// [`invoke_nudge`](Self::invoke_nudge) was.
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] when the argument is not text, and
    /// [`InvokeError::Rejected`] naming a malformed port address.
    fn invoke_begin_edit_default(
        &self,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (node, port) = parse_node_port(s).ok_or_else(|| {
            InvokeError::rejected(format!(
                "begin_edit_default: malformed argument {s:?} (expected \"<node>.<port>\")"
            ))
        })?;
        Ok(IntrospectValue::Bool(self.begin_edit_default(node, port)))
    }

    /// R1564 — the `nudge` arm's body, lifted out of
    /// [`invoke`](ExternalIntrospect::invoke) to keep that dispatch under the
    /// workspace line ceiling (this round's reasons pushed it over). Same split
    /// this file already made for [`invoke_add_node`](Self::invoke_add_node).
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] when the argument is not text, and
    /// [`InvokeError::Rejected`] naming a malformed delta pair.
    fn invoke_nudge(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (dx, dy) = parse_pair_i32(s).ok_or_else(|| {
            InvokeError::rejected(format!(
                "nudge: malformed argument {s:?} (expected \"<dx>,<dy>\")"
            ))
        })?;
        Ok(IntrospectValue::Bool(self.nudge_selected(dx, dy)))
    }

    fn invoke_cut_wires(&self, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (a, b) = parse_cut_spec(&s)
            .ok_or_else(|| InvokeError::rejected(format!("cut_wires: malformed cut spec {s:?}")))?;
        Ok(IntrospectValue::Text(csv_ids(
            self.cut_wires(a, b).iter().map(|id| id.0),
        )))
    }

    // ─── R1227 comment frames ─────────────────────────────────────────

    fn frame_by_id(&self, id: FrameId) -> Option<Node<MaterialOp>> {
        frame_node(&self.graph(), id).cloned()
    }

    /// R1227 — frame the current node selection: the bounding box of the
    /// selected nodes grown by [`FRAME_PAD`] on every side (plus
    /// [`FRAME_HEADER_H`] on top for the title strip), titled `"Comment N"`. The
    /// canonical Blueprint "comment the selection" create (the RPC `add_frame`
    /// verb + the future `C` gesture funnel here). Undoable. `None` when no node
    /// is selected (a frame with nothing to annotate is not created). The node
    /// selection is untouched — frames are a separate annotation axis, not part
    /// of [`Selection`].
    ///
    /// R1596 — membership becomes a **fact** here ([`Document::enframe`], which writes each
    /// member's [`Node::parent`]) where the editor re-derived it from the rectangle on
    /// every read. That is the DCC's model (`NODE_OT_join`) and it is what makes a member
    /// survive a resize: a rectangle test silently adopts and abandons nodes
    /// as the box is dragged, so what the frame *said* it held changed without
    /// anyone editing it. The **geometry** stays this application's — the
    /// crate deliberately has no card extent (R1589) — so the padded bounding
    /// box is written onto the frame node right after the relation is made.
    fn add_frame(&self) -> Option<FrameId> {
        let graph = self.graph();
        let selected = self.selection.get().nodes();
        let members: Vec<NodeId> = selected.iter().copied().collect();
        // R1230 — the selection's bounding box through the `node_bounds` SSOT
        // (the same fold `align_selected` uses); the pre-R1230 hand-rolled fold
        // here bypassed both `node_bounds` and the `bottom()` accessor.
        let (left, top, right, bottom) = node_bounds(
            kind_nodes(&graph)
                .map(|(node, _)| node)
                .filter(|n| selected.contains(&n.id)),
        )?;
        let x = (left - FRAME_PAD).max(0);
        let y = (top - FRAME_PAD - FRAME_HEADER_H).max(0);
        let w = (right + FRAME_PAD) - x;
        let h = (bottom + FRAME_PAD) - y;
        // R1596 — numbered by how many frames there ARE, not by a counter that
        // kept climbing over deletions. The editor's own counter was the id
        // source too, and ids are now the tree's and shared with nodes, so the
        // name and the handle are separate facts.
        let ordinal = frame_nodes(&graph).count() + 1;
        let mut made = None;
        self.edit("Add frame", None, |graph, _| {
            let Ok(enframed) = graph.enframe(TREE, &members, Some(format!("Comment {ordinal}")))
            else {
                return;
            };
            if let Some(node) = graph
                .tree_mut(TREE)
                .and_then(|t| t.node_mut(enframed.frame))
            {
                node.x = x;
                node.y = y;
                node.appearance.width = Some(upx(w));
                node.appearance.height = Some(upx(h));
            }
            made = Some(enframed.frame);
        });
        made
    }

    /// R1227 — remove comment frame `id` (the nodes it annotated stay). Undoable
    /// (one Ctrl+Z restores it with its [`FrameId`] + title). `false` for an
    /// unknown id.
    ///
    /// R1596 — the members are handed to the frame ABOVE rather than to the
    /// canvas ([`Document::remove_node`]'s `adopted`), so deleting the middle
    /// frame of a nest does not strand its contents outside the outer one.
    fn remove_frame(&self, id: FrameId) -> bool {
        if self.frame_by_id(id).is_none() {
            return false;
        }
        self.edit("Remove frame", None, |graph, sel| {
            if graph.remove_node(TREE, id).is_ok() {
                *sel = validate_after(sel.clone(), &[id], &[]);
            }
        });
        true
    }

    /// R1234 — move comment frame `id` to a new `x` (`new_x`) and / or `y`
    /// (`new_y`), carrying every node it CURRENTLY contains by the same clamped
    /// delta as ONE undo step (`GraphEdit`, label "Move frame"). The
    /// Blueprint move-with-contents contract — the membership is snapshotted at
    /// the start of the move ([`frame_contains`]), so the moved set
    /// is exactly what the paint + the `contains` query show. Nodes outside the
    /// frame are untouched. `false` for an unknown id.
    fn translate_frame(&self, id: FrameId, new_x: Option<i32>, new_y: Option<i32>) -> bool {
        let graph = self.graph();
        let Some(frame) = frame_node(&graph, id) else {
            return false;
        };
        let members: Vec<Node<MaterialOp>> = frame_members(&graph, id);
        let dx = clamp_frame_dx(frame, &members, new_x.map_or(0, |x| x - frame.x));
        let dy = clamp_frame_dy(frame, &members, new_y.map_or(0, |y| y - frame.y));
        if dx == 0 && dy == 0 {
            return true;
        }
        // R1596 — `Document::translate` moves the frame and everything it
        // CONTAINS, transitively, which is what containment means; the editor
        // moved one geometric level and a nested frame's own members stayed put.
        self.edit("Move frame", None, |graph, _| {
            let _ = graph.translate(TREE, id, dx, dy);
        });
        true
    }

    /// R1234 — resize comment frame `id` to a new `w` (`new_w`) and / or `h`
    /// (`new_h`), clamped to `[FRAME_MIN, (WORLD - origin).max(FRAME_MIN)]` so the
    /// box keeps its chrome and — whenever the origin leaves room for it — its
    /// right / bottom edge stays on the world surface. (The minimum wins over the
    /// world fit: a frame whose origin is within `FRAME_MIN` of the world edge
    /// keeps the `FRAME_MIN` chrome and its edge pokes past `WORLD` rather than
    /// collapsing below its title strip — the deliberate min-size-over-fit
    /// tradeoff.) The origin is fixed and NO node moves: a resize changes which
    /// nodes the frame contains (recomputed lazily by
    /// [`frame_contains`]), it does not drag them (`GraphEdit`,
    /// label "Resize frame"). `false` for an unknown id.
    fn resize_frame(&self, id: FrameId, new_w: Option<i32>, new_h: Option<i32>) -> bool {
        let graph = self.graph();
        let Some(frame) = frame_node(&graph, id) else {
            return false;
        };
        let w = new_w.map_or(frame.width(), |w| {
            w.clamp(FRAME_MIN, (WORLD - frame.x).max(FRAME_MIN))
        });
        let h = new_h.map_or(frame.height(), |h| {
            h.clamp(FRAME_MIN, (WORLD - frame.y).max(FRAME_MIN))
        });
        self.edit("Resize frame", None, |graph, _| {
            if let Some(node) = graph.tree_mut(TREE).and_then(|t| t.node_mut(id)) {
                node.appearance.width = Some(upx(w));
                node.appearance.height = Some(upx(h));
            }
        });
        true
    }

    /// The `add_node` verb arm (extracted to keep `invoke` within the workspace
    /// line ceiling): create a node by palette kind name, returning its new id.
    fn invoke_add_node(&mut self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let kind = PALETTE
            .iter()
            .position(|&(name, _)| name == *s)
            .ok_or_else(|| Self::unknown_kind_reason("add_node", s))?;
        let id = self
            .add_node(kind)
            .ok_or_else(|| InvokeError::rejected("add_node: the graph is at its node limit"))?;
        Ok(IntrospectValue::Int(i64::from(id.0)))
    }

    /// R1598 — swap what a node IS, keeping which node it is (the DCC's
    /// `NODE_OT_swap_node`). Answers what the swap cost, as
    /// `"<carried>|<severed>|<discarded>"` — three CSVs, so a client can tell
    /// "it worked" from "it worked and cost you a wire" without a second call.
    ///
    /// The node's id survives, which is the whole point: the DCC's operator
    /// creates a new node and deletes the old one, so a selection, a saved
    /// layout and an agent holding the id all break.
    fn swap_node(&self, id: NodeId, kind: usize) -> Option<String> {
        let &(_, op) = PALETTE.get(kind)?;
        kind_node(&self.graph(), id)?;
        let mut cost = None;
        self.edit("Swap node", None, |graph, _| {
            if let Ok(swapped) = graph.set_kind(TREE, id, op) {
                cost = Some(format!(
                    "{}|{}|{}",
                    swapped
                        .carried
                        .iter()
                        .map(|c| format!("{}>{}", c.from, c.to))
                        .collect::<Vec<_>>()
                        .join(","),
                    csv_ids(swapped.severed.iter().map(|l| l.id.0)),
                    // R1598 — the value, not just its address: the swap has
                    // already happened, so "in1 lost something" leaves a client
                    // nothing to show, while "in1 lost 0.5" does.
                    swapped
                        .discarded
                        .iter()
                        .map(|(port, value)| format!("{port}={}", value.display()))
                        .collect::<Vec<_>>()
                        .join(","),
                ));
            }
        });
        cost
    }

    /// R1598 — the `swap_node` verb arm: `"<node>,<kind name>"`.
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] for a non-text argument, and
    /// [`InvokeError::Rejected`] naming a malformed pair or an unknown kind.
    fn invoke_swap_node(&self, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (node, name) = s.split_once(',').ok_or_else(|| {
            InvokeError::rejected(format!(
                "swap_node: malformed argument {s:?} (expected \"<node>,<kind>\")"
            ))
        })?;
        let id =
            NodeId(node.trim().parse().map_err(|_| {
                InvokeError::rejected(format!("swap_node: {node:?} is not a node id"))
            })?);
        let name = name.trim();
        let kind = PALETTE
            .iter()
            .position(|&(n, _)| n == name)
            .ok_or_else(|| Self::unknown_kind_reason("swap_node", name))?;
        Ok(match self.swap_node(id, kind) {
            Some(cost) => IntrospectValue::Text(cost),
            None => IntrospectValue::Null,
        })
    }

    /// R1227 — the `add_frame` verb arm: frame the selection, returning the new
    /// id (`Null` when nothing is selected).
    fn invoke_add_frame(&mut self) -> IntrospectValue {
        match self.add_frame() {
            Some(id) => IntrospectValue::Int(i64::from(id.0)),
            None => IntrospectValue::Null,
        }
    }

    /// R1227 — the `remove_frame` verb arm: delete a frame by id (`false` on an
    /// unknown id; a non-`Int` arg is a `TypeMismatch`).
    fn invoke_remove_frame(
        &mut self,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let &IntrospectValue::Int(i) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
        Ok(IntrospectValue::Bool(self.remove_frame(NodeId(id))))
    }

    /// R1227 / R1596 — the comment-frame verbs, lifted out of
    /// [`invoke`](ExternalIntrospect::invoke) to keep that dispatch under the
    /// workspace line ceiling — the same split `query_frame` / `intervene_frame`
    /// already made for the read and the write.
    ///
    /// # Errors
    ///
    /// [`InvokeError::TypeMismatch`] for a `remove_frame` argument that is not
    /// an id.
    fn invoke_frame(
        &mut self,
        verb: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match verb {
            "add_frame" => Ok(self.invoke_add_frame()),
            "remove_frame" => self.invoke_remove_frame(args),
            "attach" => Ok(IntrospectValue::Text(csv_ids(
                self.attach_selected().iter().map(|id| id.0),
            ))),
            // Every caller is the arm that matched these four names, so a fifth
            // verb reaching here is a dispatch that named it and forgot it.
            _ => Ok(IntrospectValue::Text(csv_ids(
                self.detach_selected().iter().map(|id| id.0),
            ))),
        }
    }

    /// R1227 — the `frame.<id>.<field>` read: the comment-frame rect / title +
    /// `contains`. `None` when the path is not a frame path or the id / field is
    /// unknown.
    ///
    /// R1596 — `contains` answers the **relation** ([`frame_members`]) where it used to answer a
    /// rectangle test, and two reads join it: `parent` (the frame this frame is
    /// inside, so a nest is readable from either end) and `contents` (every
    /// descendant, which is what a drag actually carries). The DCC publishes
    /// none of the three — `bNode::parent` reaches Python as `node.parent` and there is no accessor
    /// for a frame's children at all, so its own UI code walks every node in
    /// the tree comparing pointers.
    fn query_frame(&self, path: &str) -> Option<IntrospectValue> {
        let rest = path.strip_prefix("frame.")?;
        let (id_str, field) = rest.split_once('.')?;
        let id = NodeId(id_str.parse().ok()?);
        let graph = self.graph();
        let f = frame_node(&graph, id)?;
        match field {
            "title" => Some(IntrospectValue::Text(f.title())),
            "x" => Some(IntrospectValue::Int(i64::from(f.x))),
            "y" => Some(IntrospectValue::Int(i64::from(f.y))),
            "w" => Some(IntrospectValue::Int(i64::from(f.width()))),
            "h" => Some(IntrospectValue::Int(i64::from(f.height()))),
            "contains" => Some(IntrospectValue::Text(csv_ids(
                graph.members(TREE, id).into_iter().map(|n| n.0),
            ))),
            "contents" => Some(IntrospectValue::Text(csv_ids(
                graph.contents(TREE, id).into_iter().map(|n| n.0),
            ))),
            "parent" => Some(f.parent.map_or(IntrospectValue::Null, |p| {
                IntrospectValue::Int(i64::from(p.0))
            })),
            _ => None,
        }
    }

    /// R1242 — the prefixed-path reads for [`query`](Self::query): `detail.` /
    /// `node.<id>.<field>` / `edge.<id>` / `dissolvable.<id>` / `frame.<id>.<field>`.
    /// Extracted from `query` when its arm count crossed the line ceiling (the
    /// R1227 `query_frame` extraction precedent).
    fn query_prefixed(&self, path: &str) -> Option<IntrospectValue> {
        // R916 — `detail.<field>` is a selection-relative alias for
        // `node.<selected>.<field>`: resolve the single selected node and
        // delegate to the existing absolute-addressing read. `Null` when
        // the selection is not exactly one node (no unambiguous "the"
        // node) — the R909 selection-driven detail-panel pattern.
        if let Some(field) = path.strip_prefix("detail.") {
            return Some(match self.selected_node_path(field) {
                Some(node_path) => self.query(&node_path).unwrap_or(IntrospectValue::Null),
                None => IntrospectValue::Null,
            });
        }
        if let Some(rest) = path.strip_prefix("node.") {
            let (id_str, field) = rest.split_once('.')?;
            let id = NodeId(id_str.parse().ok()?);
            let graph = self.graph();
            let (node, op) = kind_node(&graph, id)?;
            let signature = signature_of(&graph, id)?;
            return match field {
                "title" => Some(IntrospectValue::Text(node.title())),
                "x" => Some(IntrospectValue::Int(i64::from(node.x))),
                "y" => Some(IntrospectValue::Int(i64::from(node.y))),
                // R1441 — the card's laid-out height, the twin of the long-standing
                // `frame.<id>.h`. Needed to reason about a node's CENTRE, which is
                // where an edge attaches: with cards of unequal height "these two
                // line up" is a statement about centres, and recomputing the height
                // from the port count outside the model would be a second, driftable
                // definition of it.
                "h" => Some(IntrospectValue::Int(i64::from(node.height()))),
                "inputs" => Some(IntrospectValue::Int(int_of(signature.inputs.len()))),
                "outputs" => Some(IntrospectValue::Int(int_of(signature.outputs.len()))),
                // R1596 — the frame this node sits in, or `Null` on the bare
                // canvas ([`Node::parent`], R1589). Membership is now a fact the document
                // maintains, so it is readable from the member's end as well
                // as the frame's — the DCC has `node.parent` in Python and no accessor
                // for the other direction at all.
                "parent" => Some(node.parent.map_or(IntrospectValue::Null, |p| {
                    IntrospectValue::Int(i64::from(p.0))
                })),
                // R1242 — the reroute discriminator (a first-class model read, not
                // a title match — a user-renamed knot still reads true, a node
                // renamed "Reroute" reads false).
                "is_reroute" => Some(IntrospectValue::Bool(node.is_reroute())),
                // R1256 — the first-class compute identity (the rename-stable
                // `Add`/`Multiply`/... the evaluator dispatches on). The AI-first
                // answer to "what does this node compute" — the structural reads
                // (`input_types`/arity) cannot separate ops that share a
                // signature, and `title` is rewritable, so an AI enumeration
                // reads `op` to predict/verify `value`.
                "op" => Some(IntrospectValue::Text(op.name())),
                // R1257 — whether this node is an authorable SOURCE (its `value`
                // is a stored, `intervene`-able constant vs a derived output).
                "is_source" => Some(IntrospectValue::Bool(is_source(&graph, id))),
                // R898 — the typed-port read twins: CSV of the port types in port
                // order ("" for a source / sink). The arity reads stay the
                // byte-stable count contract.
                "input_types" => Some(IntrospectValue::Text(port_types_csv(&signature.inputs))),
                "output_types" => Some(IntrospectValue::Text(port_types_csv(&signature.outputs))),
                // R1255 — the node's *evaluated* output value (the compute twin
                // of the authoring reads above): topo-eval over the graph, an
                // unconnected input taking its R899 pin default and a source op
                // its constant. `Null` when a cycle in the input cone leaves the
                // value undefined (distinguish via `eval.acyclic`). A `Float`
                // reads as a float, a `Vector` (`Color`) as a `{hex,r,g,b,a}`
                // object — the same wire form as `input_default.<port>`.
                "value" => Some(cell_or_null(evaluate(&graph, id))),
                // R899 — the typed default of an input port (the write twin is
                // `intervene node.<id>.input_default.<port>`); a `Float` reads as a
                // float, a `Vector` (`Color`) as a `{hex,r,g,b,a}` object. R1260 —
                // `resolved_input.<port>` is the *debugger* read: the value that
                // actually flows in (a wired source's output coerced to the port
                // type, else the default), `Null` on a cycle upstream.
                other => {
                    if let Some(port) = other
                        .strip_prefix("input_default.")
                        .and_then(|p| p.parse::<usize>().ok())
                    {
                        input_default(&graph, id, port).map(|v| v.to_introspect())
                    } else if let Some(port) = other
                        .strip_prefix("resolved_input.")
                        .and_then(|p| p.parse::<usize>().ok())
                    {
                        // Out-of-range port -> UnknownPath (`None`); an in-range
                        // input fed by a cycle -> `Null`.
                        input_type(&graph, id, port)?;
                        Some(cell_or_null(resolve_input_value(&graph, id, port)))
                    } else {
                        None
                    }
                }
            };
        }
        if let Some(id_str) = path.strip_prefix("edge.") {
            let id = EdgeId(id_str.parse().ok()?);
            let graph = self.graph();
            let e = edge(&graph, id)?;
            return Some(IntrospectValue::Text(format!(
                "{}:{}->{}:{}",
                e.from.node, e.from.port, e.to.node, e.to.port
            )));
        }
        // R1255 — the graph-evaluation reads: pure derived introspection over
        // the authored graph (no write twin — the value is computed, not
        // stored). `eval.output` is the terminal value at the `Output` sink;
        // `eval.acyclic` tells a client whether a `null` value is a cycle vs a
        // genuinely absent node.
        if let Some(field) = path.strip_prefix("eval.") {
            let graph = self.graph();
            // R1596 — both cycle reads come off the crate's ONE derivation
            // (`Document::cycle_nodes`), so "is it acyclic" and "who is on the
            // cycle" cannot disagree. The editor had two independent walks — a
            // DFS colouring for the bool and a second one for the ids — and the
            // crate could not serve the second at all until R1596, which is what
            // starting the migration surfaced.
            let on_cycle = graph.cycle_nodes(TREE);
            return match field {
                "output" => Some(cell_or_null(eval_terminal(&graph))),
                "acyclic" => Some(IntrospectValue::Bool(on_cycle.is_empty())),
                // R1260 — LOCALISE a cycle: the ids of the nodes ON a cycle
                // (not merely downstream), so a `null` value points at the knot
                // to break. Empty for a DAG. R1262 — through the `csv_ids` SSOT
                // (the 9th id-list-CSV consumer), not a hand-rolled join.
                "cycle_nodes" => Some(IntrospectValue::Text(csv_ids(
                    on_cycle.iter().map(|id| id.0),
                ))),
                _ => None,
            };
        }
        // R1241 — the per-node dissolve-eligibility read (the twin of the
        // `dissolve_node` verb; shares the `dissolve_plan` predicate).
        if let Some(id_str) = path.strip_prefix("dissolvable.") {
            let id = NodeId(id_str.parse().ok()?);
            return Some(IntrospectValue::Bool(self.dissolvable(id)));
        }
        // R1596 — WHICH wires a dissolve would cut, beside whether it cuts any.
        if let Some(id_str) = path.strip_prefix("dissolve_severs.") {
            let id = NodeId(id_str.parse().ok()?);
            return self.dissolve_severs(id).map(IntrospectValue::Text);
        }
        // R1227 — `frame.<id>.<field>` reads.
        self.query_frame(path)
    }

    /// R1227 / R1234 — the `intervene frame.<id>.<field>` write. `title` renames
    /// through the shared [`apply_rename`] SSOT (journaled). `x` / `y` MOVE the
    /// frame + the nodes it contains ([`Self::translate_frame`] — move-with-contents);
    /// `w` / `h` RESIZE the box, origin fixed, nodes untouched ([`Self::resize_frame`]).
    /// Each rect write journals one `GraphEdit`. An unknown id is caught up
    /// front; a non-`Int` rect value is a `TypeMismatch`.
    fn intervene_frame(
        &mut self,
        path: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        let rest = path
            .strip_prefix("frame.")
            .ok_or(InterveneError::UnknownPath)?;
        let (id_str, field) = rest.split_once('.').ok_or(InterveneError::UnknownPath)?;
        let id = NodeId(id_str.parse().map_err(|_| InterveneError::UnknownPath)?);
        if self.frame_by_id(id).is_none() {
            return Err(InterveneError::UnknownPath);
        }
        match field {
            "title" => match value {
                IntrospectValue::Text(t) => {
                    if apply_rename(&self.handle(), id, &t) {
                        Ok(())
                    } else {
                        Err(InterveneError::out_of_range(
                            "a frame title cannot be blank",
                        ))
                    }
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "x" | "y" | "w" | "h" => {
                let IntrospectValue::Int(v) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                let v = i32::try_from(v).map_err(|_| InterveneError::TypeMismatch)?;
                // The id was validated above, so both funnels return `true`;
                // `x`/`y` translate (move-with-contents), `w`/`h` resize.
                match field {
                    "x" => self.translate_frame(id, Some(v), None),
                    "y" => self.translate_frame(id, None, Some(v)),
                    "w" => self.resize_frame(id, Some(v), None),
                    _ => self.resize_frame(id, None, Some(v)),
                };
                Ok(())
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// Delete node `id` and its incident edges. No reindex: every surviving
    /// node and edge keeps its stable id, so references elsewhere stay valid.
    /// R851 — the node *and* its incident edges are captured as a removed delta,
    /// so one Ctrl+Z restores the node together with every wire it carried.
    fn delete_node(&self, id: NodeId) -> bool {
        self.delete_nodes(&BTreeSet::from([id]))
    }

    /// R879 — delete a node *set* plus every incident edge as ONE
    /// [`GraphEdit`] (one undo step restores the whole group with its
    /// stable ids — the multi-select `Delete` contract). Unknown ids are
    /// skipped; an all-unknown set is a no-op `false`.
    fn delete_nodes(&self, ids: &BTreeSet<NodeId>) -> bool {
        let graph = self.graph();
        let removed_ids: Vec<NodeId> = ids
            .iter()
            .copied()
            .filter(|&id| kind_node(&graph, id).is_some())
            .collect();
        if removed_ids.is_empty() {
            return false;
        }
        let incident_ids: Vec<EdgeId> = edges(&graph)
            .iter()
            .filter(|e| ids.contains(&e.from.node) || ids.contains(&e.to.node))
            .map(|e| e.id)
            .collect();
        let label: Cow<'static, str> = if removed_ids.len() == 1 {
            Cow::Borrowed("Delete node")
        } else {
            Cow::Owned(format!("Delete {} nodes", removed_ids.len()))
        };
        self.edit(label, None, |graph, sel| {
            for &id in &removed_ids {
                let _ = graph.remove_node(TREE, id);
            }
            *sel = validate_after(sel.clone(), &removed_ids, &incident_ids);
        });
        self.clear_removed_node_interaction(&removed_ids);
        true
    }

    /// R1236 — reset the per-node interaction latches after a node removal:
    /// cancel any in-flight grab / drag, and drop an in-flight inline edit whose
    /// target is among `removed_ids` (a gone node must not keep advertising an
    /// edit via `query editing`). Extracted from [`delete_nodes`](Self::delete_nodes)
    /// when the reroute [`dissolve_node`](Self::dissolve_node) became its 2nd
    /// consumer ([[abstraction-needs-second-consumer]]), so both removal paths
    /// restore the same invariant.
    fn clear_removed_node_interaction(&self, removed_ids: &[NodeId]) {
        self.grabbed_node.set(None);
        *self.node_drag.borrow_mut() = None;
        if self
            .editing
            .get()
            .is_some_and(|a| removed_ids.contains(&a.target.node()))
        {
            self.editing.set(None);
            self.edit_buffer.set_text(String::new());
        }
    }

    /// Delete whatever is selected — the node set or an edge (the single
    /// `Selection` makes the cases exhaustive). The `Delete` key + RPC
    /// `delete_selected` share it.
    fn delete_selected(&self) -> bool {
        match self.selection.get() {
            Selection::Edge(e) => self.remove_edge(e),
            Selection::Nodes(set) => self.delete_nodes(&set),
            Selection::None => false,
        }
    }

    /// R1236 — DISSOLVE node `id`: remove it and bridge its single upstream
    /// source straight to its single downstream target, so the wire survives the
    /// removed hop (the Blueprint "delete + reconnect" / `Alt`+`Delete` on a
    /// reroute knot — the natural inverse of R1235's [`add_reroute`](Self::add_reroute)).
    /// Requires EXACTLY one incident input edge (`A -> id`) and one output edge
    /// (`id -> B`) plus a valid, non-duplicate bridge `A -> B` — always true for
    /// a reroute, whose ports share the wire's type. ONE undoable [`GraphEdit`]:
    /// the node + its two edges are removed and the bridge added together, so a
    /// single `Ctrl`+`Z` restores the hop. `false` (a no-op) for an unknown id, a
    /// non-passthrough wiring (zero or many edges either side), or a bridge that
    /// would self-loop / mistype / duplicate an existing wire — the caller falls
    /// back to a plain [`delete_node`](Self::delete_node).
    /// R1241 — the eligibility + bridge plan for dissolving node `id`: the two
    /// incident edges to remove (`A -> id`, `id -> B`) and the bridge endpoints
    /// (`A -> B`) that replace them, or `None` when the node is not a dissolvable
    /// passthrough (not exactly one incident edge on each side, or the bridge
    /// would self-loop / mistype / duplicate an existing wire). SIDE-EFFECT FREE
    /// (mints nothing), so the `dissolvable.<id>` READ and the `dissolve_node`
    /// mutation share ONE predicate — the query can never disagree with what the
    /// verb will do ([[setter-wire-returns-read-outcome]]).
    fn dissolve_plan(&self, id: NodeId) -> Option<Rewired> {
        kind_node(&self.graph(), id)?;
        // §2 #3 — the plan IS the operation, run on a copy. `dissolve` is the
        // only thing that can say what a dissolve does, so predicting it a
        // second way is exactly the drift [[setter-wire-returns-read-outcome]]
        // names; the crate's determinism is what makes the copy authoritative.
        let mut probe = self.graph();
        probe.dissolve(TREE, id).ok()
    }

    /// R1241 / R1596 — whether dissolving node `id` keeps every value flowing
    /// (the `dissolvable.<id>` read; the AI-first eligibility twin of the
    /// `dissolve_node` verb, so an editor can gray out / offer "Dissolve"
    /// without a mutate-to-probe).
    ///
    /// The predicate **widened** this round and the word it answers changed
    /// with it. The editor's own rule was "exactly one wire in and one out", a
    /// shape test that refused a two-input node whose dissolve is perfectly
    /// well-defined; [`Document::dissolve`] is the general form (R1586, the DCC's `NODE_OT_delete_reconnect`), so what
    /// is worth asking is no longer *can it* but **does it lose anything** —
    /// [`Rewired::lossless`].
    fn dissolvable(&self, id: NodeId) -> bool {
        self.dissolve_plan(id).is_some_and(|r| r.lossless())
    }

    /// R1596 — the wires a dissolve of `id` would CUT, as a CSV of edge ids.
    ///
    /// Empty exactly when [`dissolvable`](Self::dissolvable) is true, so the boolean and
    /// the reason are one derivation read two ways. The DCC's `node_internal_relink` deletes
    /// those links with `node_remove_link` and returns `void`, so nothing there can be asked
    /// what a reconnect-delete is about to throw away — the user finds out by
    /// doing it.
    fn dissolve_severs(&self, id: NodeId) -> Option<String> {
        let plan = self.dissolve_plan(id)?;
        Some(csv_ids(plan.severed.iter().map(|link| link.id.0)))
    }

    fn dissolve_node(&self, id: NodeId) -> bool {
        let Some(plan) = self.dissolve_plan(id) else {
            return false;
        };
        let removed_edges: Vec<EdgeId> = plan.removed.iter().map(|link| link.id).collect();
        self.edit("Dissolve node", None, |graph, sel| {
            if graph.dissolve(TREE, id).is_ok() {
                *sel = validate_after(sel.clone(), &[id], &removed_edges);
            }
        });
        self.clear_removed_node_interaction(&[id]);
        true
    }

    /// R1236 — dissolve the single selected node (the `Alt`+`Delete` gesture +
    /// the RPC `dissolve_selected` verb). Only a lone node selection has an
    /// unambiguous hop to dissolve; a multi-selection / edge / empty selection is
    /// a no-op `false` (the caller can plain-`delete_selected` instead).
    fn dissolve_selected(&self) -> bool {
        self.selection
            .get()
            .node()
            .is_some_and(|id| self.dissolve_node(id))
    }

    // ─── R1596 attach / detach (the frame's two gestures) ─────────────

    /// R1596 — the **innermost** frame whose rect holds node `n`'s centre.
    ///
    /// Innermost, because a nested frame sits inside its container's rect too,
    /// so "which frame did I drop this in" has one answer only once depth
    /// breaks the tie. The DCC's `node_find_frame_to_attach` walks its node list from the END and
    /// takes the first rect hit, which is z-order — so there the answer
    /// depends on the order frames were created in, and dropping a node into
    /// an inner frame lands it in the outer one whenever the outer was made
    /// later.
    fn frame_under(graph: &Graph, n: &Node<MaterialOp>) -> Option<NodeId> {
        frame_nodes(graph)
            .filter(|f| f.id != n.id && frame_contains(f, n))
            .max_by_key(|f| graph.ancestry(TREE, f.id).len())
            .map(|f| f.id)
    }

    /// R1596 — the DCC's `NODE_OT_attach`: put each selected node into the frame it is
    /// sitting on, as one undo step. Answers the nodes that changed frame.
    ///
    /// This is where the geometric question ([`frame_contains`]) becomes the
    /// membership FACT ([`Node::parent`]) — one act, at a moment the user chose,
    /// rather than a rectangle test re-run on every read. A node dropped on no
    /// frame is left where it is rather than being detached, because "attach"
    /// says what it does; taking a node out is [`detach_selected`](Self::detach_selected).
    ///
    /// Acts on [`Document::outermost`], so dragging a frame and a node inside it
    /// does not re-parent the node into a frame the frame itself is landing in.
    fn attach_selected(&self) -> Vec<NodeId> {
        let graph = self.graph();
        let selected: Vec<NodeId> = self.selection.get().nodes().into_iter().collect();
        let mut moves: Vec<(NodeId, NodeId)> = Vec::new();
        for id in graph.outermost(TREE, &selected) {
            let Some(tree) = graph.tree(TREE) else { break };
            let Some(node) = tree.node(id) else { continue };
            if let Some(frame) = Self::frame_under(&graph, node) {
                if node.parent != Some(frame) {
                    moves.push((id, frame));
                }
            }
        }
        if moves.is_empty() {
            return Vec::new();
        }
        let attached: Vec<NodeId> = moves.iter().map(|&(id, _)| id).collect();
        self.edit("Attach to frame", None, |graph, _| {
            for &(id, frame) in &moves {
                // A refusal is possible and is not an error here: dropping a
                // frame onto one of its own descendants would close a cycle, and
                // the honest outcome is that the drop does not re-parent.
                let _ = graph.set_parent(TREE, id, Some(frame));
            }
        });
        attached
    }

    /// R1596 — the DCC's `NODE_OT_detach`: take each selected node out of the frame
    /// immediately containing it, as one undo step. Answers the nodes that
    /// moved.
    ///
    /// **One level** ([`Document::unframe`]), so a node in `Outer > Inner` lands in `Outer` and repeating
    /// walks out. The DCC's operator clears the parent outright, so only the
    /// all-the-way form is reachable there.
    fn detach_selected(&self) -> Vec<NodeId> {
        let selected: Vec<NodeId> = self.selection.get().nodes().into_iter().collect();
        let mut moved = Vec::new();
        self.edit("Detach from frame", None, |graph, _| {
            moved = graph.unframe(TREE, &selected).unwrap_or_default();
        });
        moved
    }

    /// Hit-test a window-px click against every wire; the first within
    /// tolerance is the selection candidate (its stable [`EdgeId`]).
    /// R877 — `(px, py)` in graph units (the caller converts the cursor via
    /// [`cursor_graph`](Self::cursor_graph)); the hit halo is screen-constant.
    fn hit_test_edge(&self, px: f64, py: f64) -> Option<EdgeId> {
        let graph = self.graph();
        let threshold = f64::from(EDGE_HIT_THRESHOLD) / self.zoom.get();
        edges(&graph)
            .iter()
            .find(|e| {
                edge_endpoints(&graph, e)
                    .is_some_and(|(from, to)| point_near_edge(px, py, from, to, threshold))
            })
            .map(|e| e.id)
    }

    /// R948 — the selection move-loop SSOT behind nudge / align / distribute:
    /// map `target(n)` onto every node in `sel` through the clamped
    /// `set_node_pos`, inside one reactive [`batch`] (so subscribers see one
    /// atomic group move), and journal the net displacement as ONE
    /// `GraphEdit`. `coalescable` folds a contiguous same-member run — true
    /// for an arrow-nudge burst, false for a discrete align / distribute
    /// command. Returns whether any node actually moved (a fully-clamped or
    /// already-in-place run journals nothing and returns `false`). The three
    /// callers differ only in how they compute `target` ([[three-site-internal-duplication-substrate-lift]]).
    fn apply_node_moves(
        &self,
        sel: &[Node<MaterialOp>],
        coalescable: bool,
        target: impl Fn(&Node<MaterialOp>) -> (i32, i32),
    ) -> bool {
        let mut moves = Vec::with_capacity(sel.len());
        batch(|| {
            for n in sel {
                let (x, y) = target(n);
                let before = (n.x, n.y);
                if self.set_node_pos(n.id, x, y) {
                    let after = self.node_by_id(n.id).map_or(before, |m| (m.x, m.y));
                    moves.push((n.id, before, after));
                }
            }
        });
        let changed = moves.iter().any(|(_, before, after)| before != after);
        self.record_moves(moves, coalescable);
        changed
    }

    /// The selected nodes, snapshotted by value in id order — the shared
    /// preamble for the move commands (so the bbox / sort math reads a stable
    /// set while `apply_node_moves` mutates the live signal).
    fn selected_nodes(&self) -> Vec<Node<MaterialOp>> {
        let members = self.selection.get().nodes();
        let graph = self.graph();
        kind_nodes(&graph)
            .map(|(node, _)| node)
            .filter(|n| members.contains(&n.id))
            .cloned()
            .collect()
    }

    /// Nudge the selected node by `(dx, dy)` (the arrow-key path). R853 — each
    /// nudge journals a *coalescable* move, so a burst of arrow keys collapses to
    /// one undo step.
    ///
    /// R949.1 — returns whether the key was a nudge command (a non-empty
    /// selection), NOT whether the position changed. The arrow keys route here
    /// through `apply_key` and the shell falls an *unhandled* key through to
    /// `scroll_key` (canvas pan). So a nudge of a selection pinned at the world
    /// edge must still report **handled** — otherwise the clamped no-op would
    /// fall through and pan the canvas, which a nudge must not do. With no
    /// selection it reports unhandled, letting the arrow pan the canvas (the
    /// intended empty-selection behaviour). `apply_node_moves`'s own
    /// "did it move" result is for the journal, not the handled-flag.
    fn nudge_selected(&self, dx: i32, dy: i32) -> bool {
        let sel = self.selected_nodes();
        if sel.is_empty() {
            return false;
        }
        self.apply_node_moves(&sel, true, |n| (n.x + dx, n.y + dy));
        true
    }

    /// R948 — align every selected node to one edge / centre of the selection's
    /// bounding box, as ONE (non-coalescable, discrete) undo step. A no-op
    /// (`false`) on fewer than two selected nodes — a single node is already its
    /// own bbox — or when nothing actually moves. Horizontal specs touch only
    /// `x`, vertical only `y`; vertical centre / bottom use each node's own
    /// `height()` so cards of different port counts land flush.
    fn align_selected(&self, spec: AlignSpec) -> bool {
        let sel = self.selected_nodes();
        if sel.len() < 2 {
            return false;
        }
        let Some((left, top, right, bottom)) = node_bounds(sel.iter()) else {
            return false;
        };
        self.apply_node_moves(&sel, false, |n| match spec {
            AlignSpec::Left => (left, n.y),
            // R1243 — horizontal specs use each node's own `width()` (the twin of
            // the CenterV / Bottom `height()` below), so a reroute knot aligns by
            // its dot, exactly as differently-sized cards align by their own box.
            AlignSpec::CenterH => (i32::midpoint(left, right) - n.width() / 2, n.y),
            AlignSpec::Right => (right - n.width(), n.y),
            AlignSpec::Top => (n.x, top),
            AlignSpec::CenterV => (n.x, i32::midpoint(top, bottom) - n.height() / 2),
            AlignSpec::Bottom => (n.x, bottom - n.height()),
        })
    }

    /// R948 — distribute the selected nodes so their centres are equally spaced
    /// along `axis`, holding the two extreme members fixed. A no-op (`false`) on
    /// fewer than three selected nodes (two have nothing between the extremes to
    /// space, and would divide by zero). ONE discrete undo step. The i-th of N
    /// (sorted by centre) lands at `first_centre + i·(span / (N-1))`.
    fn distribute_selected(&self, axis: DistributeAxis) -> bool {
        let mut sel = self.selected_nodes();
        if sel.len() < 3 {
            return false;
        }
        sel.sort_by_key(|n| centre_key(n, axis));
        let first_c = f64::from(centre_key(&sel[0], axis));
        let last_c = f64::from(centre_key(&sel[sel.len() - 1], axis));
        let span = last_c - first_c;
        let denom = f64::from(i32::try_from(sel.len() - 1).unwrap_or(1));
        // R948 — precompute each member's target centre (key = doubled centre),
        // keyed by id so the move loop is order-independent.
        let targets: BTreeMap<NodeId, (i32, i32)> = sel
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let c2 = first_c + span * f64::from(i32::try_from(i).unwrap_or(0)) / denom;
                let pos = match axis {
                    // R1243 — the horizontal target derives x from the node's own
                    // `width()` (the `n.height()` vertical twin), so a reroute
                    // knot's centre lands on the equal-spacing grid.
                    DistributeAxis::Horizontal => {
                        (round_i32((c2 - f64::from(n.width())) / 2.0), n.y)
                    }
                    DistributeAxis::Vertical => {
                        (n.x, round_i32((c2 - f64::from(n.height())) / 2.0))
                    }
                };
                (n.id, pos)
            })
            .collect();
        self.apply_node_moves(&sel, false, |n| {
            targets.get(&n.id).copied().unwrap_or((n.x, n.y))
        })
    }

    /// R1383 — tidy the WHOLE graph into a layered (Sugiyama) left-to-right
    /// arrangement in ONE discrete undo step: data flows forward across
    /// columns with a crossing-reduced vertical order (see [`Layered`]). The
    /// AI-first peer of a node editor's "arrange" / "tidy" command (the engine
    /// Blueprint, the DCC, Substance) — no selection needed (it lays out
    /// everything), anchored at the graph's current top-left so the graph
    /// stays roughly in place instead of jumping to the origin. A no-op `false` on
    /// fewer than two nodes (a single node is already tidy) or when nothing
    /// moves (the graph is already laid out — the pass is idempotent). Routes
    /// through the same [`apply_node_moves`](Self::apply_node_moves) SSOT as align /
    /// distribute / nudge, so a single `Ctrl+Z` reverts the whole arrangement;
    /// comment frames are left in place (the align / distribute precedent).
    fn auto_layout(&self) -> bool {
        // A stable snapshot pointer — the live signal is replaced wholesale by
        // `set_node_pos`, so this held copy stays valid while `apply_node_moves`
        // mutates it (the `selected_nodes` stability discipline, no clone needed).
        let graph = self.graph();
        let snapshot: Vec<Node<MaterialOp>> =
            kind_nodes(&graph).map(|(node, _)| node.clone()).collect();
        if snapshot.len() < 2 {
            return false;
        }
        // Anchor at the current bounding-box top-left (clamped on-world) so the
        // tidied graph occupies roughly the same region it already did.
        let origin =
            node_bounds(snapshot.iter()).map_or((0, 0), |(l, t, _, _)| (l.max(0), t.max(0)));
        let targets = LAYOUT.run(&graph, TREE, origin, card_extent);
        let targets = targets.positions();
        self.apply_node_moves(&snapshot, false, |n| {
            targets.get(&n.id).copied().unwrap_or((n.x, n.y))
        })
    }

    /// R1390 — relax the WHOLE graph into a **force-directed (organic)**
    /// arrangement in ONE undo step: nodes repel, edges spring them together,
    /// annealed to a compact symmetric cluster (see [`Organic`]).
    /// The organic counterpart to [`auto_layout`](Self::auto_layout)'s layered
    /// "tidy" — the mode a pro editor offers for a cyclic or undirected topology
    /// (yEd organic, Graphviz neato). No selection needed (it lays out
    /// everything), anchored at the graph's current top-left so it stays roughly
    /// put, and routed through the same [`apply_node_moves`](Self::apply_node_moves)
    /// SSOT as align / distribute / auto-layout, so a single `Ctrl+Z` reverts the
    /// whole relaxation. A no-op `false` on fewer than two nodes or when nothing
    /// moves (the pass reads only structure, so re-running is idempotent).
    fn force_layout(&self) -> bool {
        // A stable snapshot pointer — the `auto_layout` discipline: the live
        // signal is replaced wholesale by `set_node_pos`, so this held copy stays
        // valid while `apply_node_moves` mutates it.
        let graph = self.graph();
        let snapshot: Vec<Node<MaterialOp>> =
            kind_nodes(&graph).map(|(node, _)| node.clone()).collect();
        if snapshot.len() < 2 {
            return false;
        }
        let origin =
            node_bounds(snapshot.iter()).map_or((0, 0), |(l, t, _, _)| (l.max(0), t.max(0)));
        let targets = ORGANIC.run(&graph, TREE, origin);
        let targets = targets.positions();
        self.apply_node_moves(&snapshot, false, |n| {
            targets.get(&n.id).copied().unwrap_or((n.x, n.y))
        })
    }

    /// Select a node by id (must exist). The sum type makes any prior edge
    /// selection vanish for free — no "clear the other" bookkeeping. R920 —
    /// write the selection, first committing any in-flight *panel* edit whose
    /// node is leaving the single selection. The Details panel renders only
    /// the single selected node, so a selection change that orphans a panel
    /// edit must end it (the engine commit-on-selection-change, like a blur) —
    /// otherwise the shared field would paint nowhere while `query editing` still
    /// advertised it (an introspection lie reachable on the RPC path). A
    /// *card* edit is selection-independent (its card always paints), so it is
    /// left untouched. Every user-facing selection mutator routes through
    /// here.
    fn set_selection(&self, next: Selection) {
        if let Some(active) = self.editing.get() {
            if active.surface == EditSurface::Panel && next.node() != Some(active.target.node()) {
                let text = self.edit_buffer.text();
                apply_edit_commit(&self.handle(), active.target, &text);
                self.editing.set(None);
                self.edit_buffer.set_text(String::new());
            }
        }
        self.selection.set(next);
    }

    fn select_node(&self, id: Option<NodeId>) {
        let next = id
            .filter(|id| kind_node(&self.graph(), *id).is_some())
            .map_or(Selection::None, Selection::single);
        self.set_selection(next);
    }

    /// R879 — `Ctrl`-toggle `id`'s membership, leaving the rest of the node
    /// selection intact (a prior edge selection is replaced — the sum type's
    /// node-xor-edge rule). Removing the last member collapses to `None`
    /// through the construction funnel. Unknown ids are ignored.
    fn toggle_node(&self, id: NodeId) {
        if kind_node(&self.graph(), id).is_none() {
            return;
        }
        let mut set = self.selection.get().nodes();
        toggle_member(&mut set, id);
        self.set_selection(Selection::from_nodes(set));
    }

    /// R879 — `Shift`-add `id` to the node selection (an unordered graph
    /// has no range to extend, so Shift means "add" — the engine graph
    /// convention). Unknown ids are ignored.
    fn add_node_to_selection(&self, id: NodeId) {
        if kind_node(&self.graph(), id).is_none() {
            return;
        }
        let mut set = self.selection.get().nodes();
        set.insert(id);
        self.set_selection(Selection::from_nodes(set));
    }

    /// R880 — select every node (`Ctrl`+`A` / `invoke select_all` — the
    /// editor-canvas convention). `false` on an empty graph (nothing to
    /// select, selection unchanged).
    fn select_all(&self) -> bool {
        let all: BTreeSet<NodeId> = kind_nodes(&self.graph()).map(|(n, _)| n.id).collect();
        if all.is_empty() {
            return false;
        }
        self.set_selection(Selection::from_nodes(all));
        true
    }

    /// R880 — apply a completed marquee: every node whose card intersects the
    /// graph-space rect joins the hit set (the toolkit rubber-band / the
    /// engine intersects semantics — touching counts). The release modifiers
    /// pick the area form of the R879 click policy through the framework [`SelectionChord`]
    /// decode (R880.1 — *extend* on an unordered canvas means union): plain
    /// *replaces* the selection with the hit set (an empty sweep clears — the
    /// background-click deselect generalised to an area), `Ctrl` *toggles* each
    /// hit member, `Shift` *unions* the hit set in.
    fn apply_marquee(&self, rect: MarqueeRect, mods: Modifiers) {
        let (x0, y0, x1, y1) = rect;
        self.apply_region(
            &Region::span(x0.into(), y0.into(), x1.into(), y1.into()),
            mods,
        );
    }

    /// R1592 — apply ANY [`Region`] as an area selection, in **graph units**.
    ///
    /// The hit test used to be four inequalities written here. It is now the
    /// framework predicate (R1591), which is why that type is signed and
    /// `Rect`-free: a marquee on a panned canvas selects in world coordinates,
    /// which go negative, and a shape predicate has no business knowing what
    /// its numbers mean. Migrating it is what made the second shape and the
    /// second FIT reachable at all — `select_lasso` and `select_circle` are now
    /// this same call with a different value.
    ///
    /// A card's extent is passed as `x..=right()` — its far edge INCLUDED —
    /// which is the "touching counts" rule this marquee has stated since R880
    /// and the toolkit's rubber band shares. Stating it here rather than in an
    /// inequality is the point: it is now a sentence about the card, not an
    /// off-by-one in a filter.
    fn apply_region(&self, region: &Region, mods: Modifiers) {
        let graph = self.graph();
        let hit: BTreeSet<NodeId> = kind_nodes(&graph)
            .map(|(node, _)| node)
            .filter(|n| {
                let (min, max) = card_span(n);
                region.covers_span(min, max, RegionFit::Intersects)
            })
            .map(|n| n.id)
            .collect();
        let next = match SelectionChord::from_modifiers(mods) {
            SelectionChord::Toggle => {
                let mut set = self.selection.get().nodes();
                for id in hit {
                    toggle_member(&mut set, id);
                }
                set
            }
            SelectionChord::Extend => {
                let mut set = self.selection.get().nodes();
                set.extend(hit);
                set
            }
            SelectionChord::Replace => hit,
        };
        self.set_selection(Selection::from_nodes(next));
    }

    /// R1592 — `select_lasso` / `select_circle`: parse a shape in **graph
    /// units** and apply it as an area selection.
    ///
    /// The whole of what this application supplies is the spelling. The
    /// geometry, the closure of a lasso, the even-odd interior and the refusal
    /// of a shape that bounds no area are all
    /// [`pinion_core::region`], and the selection policy
    /// is [`Self::apply_region`] — the same one the pointer marquee uses, so a
    /// lasso and a rubber band cannot disagree about what "selected" means.
    fn invoke_region_select(
        &self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(raw) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let number = |token: &str| -> Result<i64, InvokeError> {
            token
                .trim()
                .parse::<i64>()
                .map_err(|_| InvokeError::rejected(format!("{token:?} is not a number")))
        };
        let region = if path == "select_circle" {
            let parts: Vec<&str> = raw.split(',').collect();
            let [cx, cy, r] = parts.as_slice() else {
                return Err(InvokeError::rejected(format!(
                    "malformed argument {raw:?} (expected \"<x>,<y>,<r>\")"
                )));
            };
            let radius = u32::try_from(number(r)?.max(0)).unwrap_or(u32::MAX);
            Region::circle(number(cx)?, number(cy)?, radius)
        } else {
            let mut points = Vec::new();
            for pair in raw.split(';').filter(|p| !p.trim().is_empty()) {
                let (x, y) = pair.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "malformed vertex {pair:?} (expected \"<x>,<y>\")"
                    ))
                })?;
                points.push((number(x)?, number(y)?));
            }
            Region::lasso(points)
        };
        // Refused rather than answered with zero: "your lasso was two points"
        // and "the sweep took nothing" are different facts, and only one of
        // them is the user's mistake.
        region
            .validate()
            .map_err(|err| InvokeError::rejected(err.to_string()))?;
        self.apply_region(&region, Modifiers::default());
        let took = self.selection.get().nodes().len();
        Ok(IntrospectValue::Int(
            i64::try_from(took).unwrap_or(i64::MAX),
        ))
    }

    /// R879 — the `intervene selected_ids` arm: a CSV of node ids replaces
    /// the selection as a set ("" clears). Strict: every id must exist
    /// (OutOfRange otherwise — the selection is unchanged), mirroring the
    /// single `selected` slot's must-exist filter; the write twin of the
    /// `selected_ids` query.
    fn intervene_selected_ids(&self, value: &IntrospectValue) -> Result<(), InterveneError> {
        let IntrospectValue::Text(csv) = value else {
            return Err(InterveneError::TypeMismatch);
        };
        let trimmed = csv.trim();
        if trimmed.is_empty() {
            self.set_selection(Selection::None);
            return Ok(());
        }
        let mut members = BTreeSet::new();
        for token in trimmed.split(',') {
            let raw: u32 = token
                .trim()
                .parse()
                .map_err(|_| InterveneError::TypeMismatch)?;
            let id = NodeId(raw);
            if kind_node(&self.graph(), id).is_none() {
                return Err(InterveneError::out_of_range(format!(
                    "no node {raw} in this graph"
                )));
            }
            members.insert(id);
        }
        self.set_selection(Selection::from_nodes(members));
        Ok(())
    }

    /// Select an edge by id (must exist).
    fn select_edge(&self, id: Option<EdgeId>) {
        let next = id
            .filter(|id| edge(&self.graph(), *id).is_some())
            .map_or(Selection::None, Selection::Edge);
        self.set_selection(next);
    }

    /// R878 / R901 — begin an inline edit of `target` (a node title or an input
    /// port default): validate the target exists, commit any in-flight edit of
    /// a *different* target first (the toolkit item-view discipline — an open
    /// editor commits when another item enters edit; without it the migration
    /// would silently discard the typed text), flag [`use_active_edit`], seed the shared
    /// field with the target's current text (caret parked at the end — the
    /// todomvc `begin_edit` UX), and hand focus to the field through the focus-request
    /// mailbox.
    fn begin_edit(&self, target: EditTarget, surface: EditSurface) -> bool {
        // R1246 — a reroute knot paints as a compact dot with NO card
        // ([`view_reroute_knot`] renders no header / port rows), so a CARD-surface
        // inline editor on a knot would arm a focused a11y textbox with no painted
        // peer (the paint==a11y gate). Refuse it at the ROOT — one guard covering
        // every entry point (`begin_rename` via F2 / the `begin_rename` RPC verb /
        // double-click, and `begin_edit_default` on an unwired knot pin). The
        // Details PANEL surface is unaffected: the panel row paints its own field,
        // so a knot's properties stay editable there.
        if surface == EditSurface::Card && self.is_reroute_node(target.node()) {
            return false;
        }
        let Some(seed) = self.edit_seed_text(target) else {
            return false;
        };
        let prev = self.editing.get();
        // R920 — moving the SAME target between surfaces (card title <-> the panel
        // Title row) is a *migration*: keep the in-flight buffer, just relocate the
        // field. Only a fresh open / a same-surface re-open reseeds from the model
        // (the R878 todomvc restart-editing UX); committing the buffer on a
        // surface move would be the very silent-discard the migration-commit guards.
        let migrate_surface = prev.is_some_and(|p| p.target == target && p.surface != surface);
        // R901 — opening a *different* target commits the in-flight one first
        // (the toolkit item-view discipline), so the migration never
        // silently drops text.
        if let Some(prev) = prev {
            if prev.target != target {
                let text = self.edit_buffer.text();
                apply_edit_commit(&self.handle(), prev.target, &text);
            }
        }
        self.editing.set(Some(ActiveEdit { target, surface }));
        if !migrate_surface {
            self.edit_buffer.seed(seed);
        }
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
    }

    /// R901 — the text the shared field is seeded with for `target`: a title's
    /// current name, or a port default's [`CellValue::edit_text`] (the
    /// round-trip inverse of the commit's `CellKind::parse`). `None` when the
    /// node / port is absent (the begin-edit validity gate).
    fn edit_seed_text(&self, target: EditTarget) -> Option<String> {
        let graph = self.graph();
        let id = target.node();
        let (node, _) = kind_node(&graph, id)?;
        Some(match target {
            EditTarget::Title(_) => node.title(),
            EditTarget::PortDefault { port, .. } => input_default(&graph, id, port)?.edit_text(),
            EditTarget::PosX(_) => node.x.to_string(),
            EditTarget::PosY(_) => node.y.to_string(),
            // R1264 — seed with the source constant's round-trip text (`None`
            // when the node is not a source, the begin-edit validity gate).
            EditTarget::SourceConst(_) => source_const(&graph, id)?.edit_text(),
        })
    }

    /// R878 — open the inline editor on `id`'s title (the F2 / double-click /
    /// `invoke begin_rename` entry). A thin [`Self::begin_edit`] wrapper that
    /// keeps the established `begin_rename` verb name.
    fn begin_rename(&self, id: NodeId) -> bool {
        self.begin_edit(EditTarget::Title(id), EditSurface::Card)
    }

    /// R901 — open the inline editor on node `node`'s input port `port`'s
    /// default (the double-click-on-pin-default / `invoke begin_edit_default`
    /// entry).
    ///
    /// R901.1 (session-review audit) — reject a **wired** port: the inline
    /// editor's anchor is the pin's default LABEL, which paints only for an
    /// unwired port ([`view_node`]'s `!wired_inputs` gate; a wired pin draws no
    /// label, its value comes from the edge). Opening it on a wired port would
    /// paint nothing yet steal focus and make the a11y tree advertise a textbox
    /// with no painted peer (paint gate != a11y gate — the R873/R874
    /// one-gate violation). A wired port's retained default stays settable via
    /// `intervene input_default.<port>`, which is label-independent.
    fn begin_edit_default(&self, node: NodeId, port: usize) -> bool {
        if self.input_wired(node, port) {
            return false;
        }
        self.begin_edit(EditTarget::PortDefault { node, port }, EditSurface::Card)
    }

    /// R1264 — open the inline editor on `id`'s authored output constant (the
    /// double-click-on-source-value / `invoke begin_edit_value` entry). The
    /// output-side twin of [`Self::begin_edit_default`]. Rejects a **non-source**
    /// node: only a source paints an `oconst_` label to anchor the field, so
    /// opening it on a compute op / sink / reroute would steal focus and advertise
    /// an a11y textbox with no painted peer (the same paint==a11y gate the
    /// R901.1 wired-port reject guards). A compute op's / sink's `value` is
    /// derived — settable only via wiring, not this editor.
    fn begin_edit_source_value(&self, id: NodeId) -> bool {
        if !is_source(&self.graph(), id) {
            return false;
        }
        self.begin_edit(EditTarget::SourceConst(id), EditSurface::Card)
    }

    /// R918 — open the inline editor on the Details panel's `field` row of the
    /// *single selected* node (a panel-row click via the `detail_<field>` wire,
    /// or `invoke begin_edit_detail`). The field key matches the `detail_<key>`
    /// row tag and the `detail.<field>` query alias: `title`, `x`, `y`, or
    /// `in_<port>`. `false` when nothing is selected, the selection is not a
    /// single node, or the field key is unknown / the port is absent.
    ///
    /// Unlike the card's `begin_edit_default`, a *wired* port is editable here:
    /// the panel row always paints its default value (the canvas pin label is
    /// the only thing wiring hides), so the field has a painted anchor either
    /// way — no R901.1 phantom-textbox risk.
    fn begin_edit_detail(&self, field: &str) -> bool {
        let Some(id) = self.selection.get().node() else {
            return false;
        };
        let Some(target) = detail_edit_target(id, field) else {
            return false;
        };
        self.begin_edit(target, EditSurface::Panel)
    }

    /// R901.1 — whether an edge feeds node `node`'s input `port`. The wired-pin
    /// predicate that gates the inline editor to the same ports whose default
    /// label paints, so `editing` can never point at an unpainted field.
    fn input_wired(&self, node: NodeId, port: usize) -> bool {
        edges(&self.graph())
            .iter()
            .any(|e| e.to == Socket::new(node, uport(port)))
    }

    /// Pointer `send` wire (the same channel the router and RPC share).
    fn handle_send(&mut self, payload: &str) -> IntrospectValue {
        // Decode via the canonical send-wire SSOT (`split_send_payload`):
        // a composite `"key:event[:mods]"` yields `(Some(key), event)`; a bare
        // `"event"` (canvas background) yields `(None, event)`.
        // R879 — the R781 third wire segment (held modifiers) now matters:
        // `Ctrl`+release toggles membership, `Shift`+release adds.
        // R880 — the empty-key wire `":event:mods"` is a *background* event
        // with held modifiers (this coordinator opts in via
        // `wants_bare_send_modifiers`, so a `Ctrl`/`Shift` marquee release
        // carries its token).
        let (sub, event, mods) = match split_send_payload(payload) {
            Some(("", event, mods)) => (None, event, mods),
            Some((key, event, mods)) => (Some(key), event, mods),
            None => (None, payload, Modifiers::empty()),
        };
        match (sub, event) {
            (Some(s), "PointerDown") => {
                // R1220 — a press OUTSIDE the create menu dismisses it
                // (click-away); a press on a `create_<idx>` card (commit) or the
                // menu's own chrome (`pin_menu`) leaves it open.
                if parse_create_sub(s).is_none() && s != PIN_MENU_SUB {
                    self.cancel_pin_create();
                }
                if parse_node_sub(s).is_some() {
                    self.grabbed_node.set(parse_node_sub(s));
                    *self.node_drag.borrow_mut() = None;
                    self.pending_press.set(PendingPress::NodeBody);
                } else if let Some((n, j)) = parse_oport_sub(s) {
                    self.pending_press.set(PendingPress::OutputPort(n, j));
                } else if let Some(kind) = parse_palette_sub(s) {
                    // R850 — a palette card press: suppresses the background
                    // edge-probe. R1411 — carries the pressed PALETTE index so
                    // `begin_drag` can arm a drag-to-instantiate session (a drop
                    // on the canvas creates that node at the drop point) while a
                    // press-release IN PLACE still adds it at the spawn point on
                    // the matching PointerUp.
                    self.pending_press.set(PendingPress::Palette(kind));
                } else if parse_create_sub(s).is_some() {
                    // R1220 — a pin-drop create-menu card press: like a palette
                    // press, its commit (the auto-wire) runs on the matching
                    // PointerUp; its own variant so the release never misreads it.
                    self.pending_press.set(PendingPress::CreateMenu);
                } else if parse_detail_sub(s).is_some() {
                    // R918 — a Details-panel row press: like a palette press, the
                    // inline-editor open runs on the matching PointerUp.
                    self.pending_press.set(PendingPress::DetailRow);
                } else if let Some((n, i)) = parse_iport_sub(s) {
                    // R1174 — an input-port press, recorded with its identity so
                    // a wired input can arm a reconnect drag in `begin_drag`.
                    // Distinct from a background press, so it never triggers the
                    // edge-click probe.
                    self.pending_press.set(PendingPress::InputPort(n, i));
                } else {
                    // Any other in-card press (e.g. an unwired pin's default
                    // label): inert, but still suppresses the edge-click probe
                    // (a press inside a node is not an empty-canvas click).
                    self.pending_press.set(PendingPress::Inert);
                }
            }
            (Some(s), "PointerUp") => {
                // R849 — a palette card's release creates a node at the spawn
                // point; a node card's release selects it. R1411 — this is the
                // CLICK (press-release in place) path only: a palette card that
                // was DRAGGED onto the canvas committed at the drop point in
                // `drag_release`, and the router's `!became_drag` gate
                // (input.rs) suppresses this trailing click for a real drag, so
                // the two creation gestures are mutually exclusive (never a
                // double-add).
                if let Some(kind) = parse_palette_sub(s) {
                    self.add_node(kind);
                } else if let Some(kind) = parse_create_sub(s) {
                    // R1220 — a create-menu card release commits that kind
                    // through the auto-wire path (node + edge, one undo step).
                    self.commit_pin_create_kind(kind);
                } else if let Some(field) = parse_detail_sub(s) {
                    // R918 — a Details-panel row release opens the inline editor
                    // on the selected node's matching property (surface = Panel).
                    self.begin_edit_detail(field);
                } else if let Some(n) = parse_node_sub(s) {
                    // R879 — modifier-aware select (the R781 wire segment,
                    // decoded through the framework `SelectionChord` policy SSOT (R880.1)). The
                    // capture path delivers this release even after a real
                    // move (unlike the router's routed-click suppression,
                    // R876), so a *moved* gesture skips the selection
                    // mutation — a group drag must not collapse the set it
                    // dragged.
                    if !self.gesture_moved() {
                        match SelectionChord::from_modifiers(mods) {
                            SelectionChord::Toggle => self.toggle_node(n),
                            SelectionChord::Extend => self.add_node_to_selection(n),
                            SelectionChord::Replace => self.select_node(Some(n)),
                        }
                    }
                }
                self.end_gesture();
            }
            (None, "PointerUp") => {
                // Background release. A *live* marquee applies its rect-hit
                // set (modifier-aware: plain replaces, `Ctrl` toggles,
                // `Shift` unions — the click policy's area form); an
                // in-place click selects the edge the capture-seed probe
                // landed on, else deselects everything. Only when no node
                // drag was armed.
                if self.grabbed_node.get().is_none() {
                    if let Some(rect) = self.live_marquee_rect() {
                        // A MOVED background gesture is a marquee — never a
                        // splice, even if a double-click armed one (the R1243
                        // double-click-then-drag case): the armed splice is
                        // dropped by `end_gesture` below.
                        self.apply_marquee(rect, mods);
                    } else if let Some(edge) = self.pending_reroute_splice.get() {
                        // R1243 — an IN-PLACE double-click on a wire splices a
                        // reroute knot (deferred here from the `DoubleClick` arm).
                        // `add_reroute` selects the new knot.
                        self.add_reroute(edge);
                    } else {
                        match self.pending_edge_hit.get() {
                            Some(e) => self.select_edge(Some(e)),
                            None => self.select_node(None),
                        }
                    }
                }
                self.end_gesture();
            }
            (None, "PointerDown") => {
                // R1220 — a background press on empty canvas dismisses an open
                // create menu (click-away), then starts a clean gesture.
                self.cancel_pin_create();
                // R880.1 — a fresh background press starts from a clean
                // slate via the FULL gesture reset (defensive: a leaked
                // anchor / grabbed node / painted band from a lost release
                // must not corrupt this gesture — a half-clear left the
                // stale rubber band painted and a stale `grabbed_node`
                // teleporting its node onto the new press).
                self.reset_gesture();
            }
            // R880.1 — the router revokes the gesture (touch cancel, system
            // gesture steal): revert any live drag's members to their press
            // positions and drop every latch WITHOUT journaling (a cancelled
            // gesture never happened — `reset_gesture`, not `end_gesture`).
            // Reaches here on both wires: bare ("PointerCancel", background
            // marquee) and composite ("node_3:PointerCancel", node drag).
            (_, "PointerCancel") => {
                if let Some(start) = self.node_drag.borrow().as_ref() {
                    batch(|| {
                        for &(id, _, _, px, py) in &start.members {
                            self.set_node_pos(id, px, py);
                        }
                    });
                }
                self.preview.set(None);
                self.reset_gesture();
            }
            // R878 / R901 — a double-click opens the inline editor: on a node
            // card it edits the title (the R664/R790 todomvc dblclick-to-edit
            // idiom), on a pin's default label it edits that port's default
            // value (R901 — the same disambiguation a node card uses: a single
            // press selects / connects, a double-click edits). The router's W3C
            // dblclick detection synthesises this on the second in-place press,
            // and the `scene/double_click` RPC drain emits the identical wire.
            (Some(s), "DoubleClick") => {
                if let Some(n) = parse_node_sub(s) {
                    // R1246 — begin the card-title rename. `begin_edit` refuses a
                    // CARD edit on a reroute KNOT (it paints no card to host the
                    // field), so a knot double-click is a clean no-op here — NOT a
                    // destructive dissolve (R1245 bound double-click-to-dissolve,
                    // an invented, non-standard, footgun gesture that duplicated
                    // the standard `Alt`+`Delete`; reverted). Dissolve stays on
                    // `Alt`+`Delete` / `invoke dissolve_node`.
                    self.begin_rename(n);
                } else if let Some((n, port)) = parse_idefault_sub(s) {
                    self.begin_edit_default(n, port);
                } else if let Some(n) = parse_oconst_sub(s) {
                    // R1264 — a double-click on a source node's output-constant
                    // label opens its inline value editor (the output-side twin
                    // of the R901 pin-default double-click).
                    self.begin_edit_source_value(n);
                }
            }
            // R1243 — a double-click on empty canvas that lands on a WIRE splices
            // a reroute knot into it (the Blueprint double-click-to-reroute
            // gesture; the AI-first twin of `invoke add_reroute <edge_id>`). The
            // press carrying this `DoubleClick` just seeded the background
            // edge-hit probe (`pending_edge_hit`) at the double-click point — the
            // same probe a single background click reads to select a wire (r880
            // (G)) — so it names the wire under the cursor. A double-click on
            // truly empty canvas seeded `None` and no-ops. The probe is CONSUMED
            // (the edge it named is about to be removed), so the gesture's
            // trailing `PointerUp` reads `None` and cannot re-select a dead edge.
            (None, "DoubleClick") => {
                // Arm the splice for the wire under the double-click (the probe
                // the press just seeded). It FIRES on the trailing in-place
                // `PointerUp`, not here — so a double-click that turns into a
                // drag (a marquee begun on a wire) marquees instead of splicing.
                self.pending_reroute_splice.set(self.pending_edge_hit.get());
            }
            _ => {}
        }
        IntrospectValue::Null
    }

    /// R879 — whether the in-flight node gesture became a drag: the SAME
    /// `live` latch that gates the members' movement (one predicate — the
    /// nodes cannot have moved without it, and a release after it must not
    /// select). Distinguishes a click (select on release) from a drag
    /// (selection untouched) on the capture path, where the release always
    /// reaches `handle_send`; measured by the framework [`DragLatch`]
    /// contract predicate, so this binding and the router can never
    /// disagree on what a click is.
    fn gesture_moved(&self) -> bool {
        self.node_drag
            .borrow()
            .as_ref()
            .is_some_and(|start| start.latch.live())
    }

    /// R880 — the marquee rect to apply at a background release: `Some` only
    /// once the gesture latched live (the SAME latch that publishes the
    /// rubber-band paint — one predicate, so "a marquee is visible" and "the
    /// release applies an area" can never disagree; the in-place click path
    /// runs otherwise).
    fn live_marquee_rect(&self) -> Option<MarqueeRect> {
        if self
            .marquee
            .borrow()
            .as_ref()
            .is_some_and(|m| m.latch.live())
        {
            self.marquee_rect.get()
        } else {
            None
        }
    }

    fn end_gesture(&self) {
        // R853 — a completed node-body drag (a node was grabbed and a drag
        // snapshot taken) journals as ONE non-coalescable move: before = the grab
        // position, after = the release position. The intermediate `pointer_move`
        // writes are live preview only. Edge-connect drags arm `pending_press`,
        // not `grabbed_node`, so they never record a move here.
        if self.grabbed_node.get().is_some() {
            if let Some(start) = self.node_drag.borrow().as_ref() {
                let moves: Vec<NodeMove> = start
                    .members
                    .iter()
                    .filter_map(|&(id, _, _, px, py)| {
                        self.node_by_id(id).map(|n| (id, (px, py), (n.x, n.y)))
                    })
                    .collect();
                let dragged = !moves.is_empty();
                self.record_moves(moves, false);
                // R1596 — a node dropped onto a frame JOINS it (the DCC runs
                // the same attach out of its transform's `special_aftertrans_update`). Only after a
                // real move: a click that grabbed and released in place must
                // not silently re-parent, and only then can attach be its own
                // undo step without a stray one behind every click.
                if dragged {
                    self.attach_selected();
                }
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
        // R1183 — the auto-pan rim probe rides inside `NodeDragStart`, so
        // dropping the drag drops the probe with it (no separate clear, and no
        // stale-cursor risk at the other `node_drag = None` sites).
        *self.node_drag.borrow_mut() = None;
        self.pending_press.set(PendingPress::None);
        self.pending_edge_hit.set(None);
        // R1243 — drop any armed reroute splice: a fresh gesture never inherits
        // one, and a double-click-then-drag's marquee release lands here to
        // cancel the splice it armed (a moved gesture is never a splice).
        self.pending_reroute_splice.set(None);
        // R880 — drop the marquee anchor + rubber-band paint (equality-skip
        // makes the idle-path clear a no-op repaint-wise).
        *self.marquee.borrow_mut() = None;
        self.marquee_rect.set(None);
    }
}

impl core::fmt::Debug for NodeGraphExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NodeGraphExternal")
            .field("nodes", &self.node_count())
            .field("edges", &edges(&self.graph()).len())
            .field("selection", &self.selection.get())
            .finish_non_exhaustive()
    }
}

impl External for NodeGraphExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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

    /// R880 — opt into the bare-target modifier wire: a background release
    /// arrives as `":PointerUp:<token>"` when modifiers are held, so the
    /// `Ctrl` / `Shift` marquee can union / toggle against the existing
    /// selection (the same R781 token the composite node-click already
    /// decodes).
    fn wants_bare_send_modifiers(&self) -> bool {
        true
    }

    /// Normalise the captured cursor against the whole canvas (the primary
    /// `node_graph` rect = the window). The canvas does not move when a node
    /// does, so it is the stable pixel reference (R786 column-resize idiom).
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Tag(GRAPH_TAG)
    }

    /// Capture-drag a node. `x_rel` / `y_rel` are the cursor's fraction across
    /// the canvas; the first move snapshots the graph-space grab offset, each
    /// later move re-derives `node = cursor_graph + grab` against the current
    /// viewport (R877 — robust to a pan / zoom landing mid-drag).
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let (gx, gy) = self.cursor_graph(f64::from(x_rel), f64::from(y_rel));
        // The cursor in screen px (the dead-zone metric space — the same
        // logical-pixel space the router's click-vs-drag latch measures).
        let screen = (
            f64::from(x_rel) * f64::from(WIN_W),
            f64::from(y_rel) * f64::from(WIN_H),
        );
        let Some(node) = self.grabbed_node.get() else {
            // Not dragging a node. A background press drives the marquee
            // gesture; every other non-drag press (port / palette) is
            // excluded via its `PendingPress` variant.
            if self.pending_press.get() == PendingPress::None {
                let mut marquee = self.marquee.borrow_mut();
                match marquee.as_mut() {
                    None => {
                        // The R51.35 capture seed (the press cursor): anchor
                        // the marquee and probe for an edge under the click
                        // so an in-place background `PointerUp` can select it.
                        *marquee = Some(MarqueeStart {
                            latch: DragLatch::new(screen),
                            press_graph: (gx, gy),
                        });
                        self.pending_edge_hit.set(self.hit_test_edge(gx, gy));
                    }
                    Some(start) => {
                        // R880 — the dead zone: the SAME framework latch the
                        // node drag and the router advance (a jittery
                        // background click stays a click: edge-select /
                        // deselect, no marquee).
                        if !start.latch.advance(screen) {
                            return;
                        }
                        // Live rubber band: publish the normalised
                        // graph-space corners for the view fn's paint.
                        self.marquee_rect
                            .set(Some(corner_rect(start.press_graph, (gx, gy))));
                    }
                }
            }
            return;
        };
        let snapshot_needed = self.node_drag.borrow().is_none();
        if snapshot_needed {
            // R879 — first capture move (the capture-seed forwards the press
            // cursor): snapshot the dragged member set. Grabbing a *selected*
            // node drags the whole selection rigidly (per-member graph-space
            // grab anchors); grabbing an unselected node drags just it,
            // leaving the selection untouched (the engine / canvas view
            // convention).
            let selection = self.selection.get();
            let members: BTreeSet<NodeId> = if selection.contains_node(node) {
                selection.nodes()
            } else {
                BTreeSet::from([node])
            };
            let snapshot: Vec<(NodeId, f64, f64, i32, i32)> = members
                .iter()
                .filter_map(|id| {
                    self.node_by_id(*id)
                        .map(|n| (*id, f64::from(n.x) - gx, f64::from(n.y) - gy, n.x, n.y))
                })
                .collect();
            if !snapshot.is_empty() {
                *self.node_drag.borrow_mut() = Some(NodeDragStart {
                    members: snapshot,
                    latch: DragLatch::new(screen),
                    // R1183 — seed the auto-pan rim probe with the press cursor;
                    // each latched move below refreshes it.
                    cursor: Cell::new((f64::from(x_rel), f64::from(y_rel))),
                });
            }
            return;
        }
        // R879 audit fix — the dead zone: nothing moves until the framework
        // `DragLatch` (the SAME predicate the router applies to routed
        // clicks / DnD) latches past the press point (the toolkit
        // `startDragDistance`; this is the capture-path twin).
        {
            let mut start = self.node_drag.borrow_mut();
            if let Some(start) = start.as_mut() {
                if !start.latch.advance(screen) {
                    return;
                }
            }
        }
        let start = self.node_drag.borrow();
        if let Some(start) = start.as_ref() {
            // R1183 — refresh the auto-pan rim probe (co-located in the drag) so
            // the driver can keep scrolling toward the rim once the cursor is
            // pinned at the window edge and no further `pointer_move` fires.
            start.cursor.set((f64::from(x_rel), f64::from(y_rel)));
            // Live preview: every member re-derives from the *current*
            // cursor + its own grab anchor (zoom/pan-robust, R877); the
            // per-frame writes batch into one atomic group move (the shared
            // [`follow_members`] kernel the auto-pan tick also runs).
            batch(|| follow_members(&self.document, &start.members, gx, gy));
        }
    }

    /// R877 §5.15 §5.49 — the canvas wheel vocabulary, riding the router's
    /// External-first offer:
    ///
    /// * `Ctrl`+wheel — zoom, anchored at the cursor (consumed).
    /// * `Shift`+wheel — horizontal pan: the vertical notches drive the
    ///   x offset, the browser/the design tool convention (consumed; written
    ///   straight onto the shared [`ScrollState`]).
    /// * plain wheel — **declined** (`false`): the router's pre-R877
    ///   scroll fallback pans the world `ScrollNode` natively, so the
    ///   2-D pan costs this binding zero code.
    ///
    /// The rel coordinates are normalised over the `GRAPH_TAG` canvas
    /// rect; a wheel routed here from a palette card (composite
    /// `node_graph#palette_*` shares the primary) lands outside `[0, 1]`
    /// and is declined — the R799 bounds-guard discipline.
    fn wheel(&mut self, x_rel: f32, y_rel: f32, dx: f32, dy: f32, modifiers: Modifiers) -> bool {
        if !(0.0..=1.0).contains(&x_rel) || !(0.0..=1.0).contains(&y_rel) {
            return false;
        }
        if modifiers.command_key() {
            let factor = ZOOM_STEP.powf(-f64::from(dy) / f64::from(LINE_HEIGHT_PX));
            let sx = f64::from(x_rel) * f64::from(WIN_W);
            let sy = f64::from(y_rel) * f64::from(WIN_H);
            self.set_zoom_anchored(self.zoom.get() * factor, sx, sy);
            return true;
        }
        if modifiers.shift_key() {
            self.scroll
                .scroll_by(round_i32(f64::from(dy) + f64::from(dx)), 0);
            return true;
        }
        false
    }

    /// Arm an edge drag from an output port (the R742 drag substrate). The
    /// payload carries the source `(node, port)`; `None` for any other press,
    /// so a node-body press falls through to the capture-drag move path.
    fn begin_drag(&self) -> Option<DragPayload> {
        // R1411 — a palette card is a drag source: dragging it onto the canvas
        // instantiates its node at the drop point (`drag_release`). The payload
        // value is the node title as Text, which the shell's generic drag-image
        // follower (R1113) surfaces as the chip label automatically. No wire
        // preview is armed — that is the port-drag path below, whose loose end
        // follows a hovered pin; a palette drag's feedback is the title chip.
        if let PendingPress::Palette(kind) = self.pending_press.get() {
            let &(title, ..) = PALETTE.get(kind)?;
            return Some(DragPayload {
                kind: Cow::Borrowed(PALETTE_DRAG_KIND),
                value: IntrospectValue::Text(title.to_string()),
            });
        }
        let (from_node, from_port, reconnect) = match self.pending_press.get() {
            // A fresh wire pulled from an output port (the R742/R838 connect).
            PendingPress::OutputPort(n, j) => (n, j, None),
            // R1174 — grab a *wired* input and pull it loose: a reconnect drag.
            // The loose end follows from the existing edge's source output (the
            // same anchored-output shape as a connect), so the preview / payload
            // substrate is reused verbatim; the edge id rides in `reconnect` so
            // `drag_release` commits through `reconnect_edge`, not `add_edge`.
            // An *unwired* input has no edge to grab — `find` yields `None` and
            // the press stays a non-drag (it never reaches the connect path).
            PendingPress::InputPort(n, i) => {
                let graph = self.graph();
                let wire = edges(&graph)
                    .iter()
                    .find(|e| e.to == Socket::new(n, uport(i)))?;
                (wire.from.node, uidx(wire.from.port), Some(wire.id))
            }
            _ => return None,
        };
        self.preview.set(Some(Preview {
            from_node,
            from_port,
            to: None,
            reconnect,
        }));
        Some(DragPayload {
            kind: Cow::Borrowed("node-edge"),
            value: IntrospectValue::Text(format!("{from_node}_{from_port}")),
        })
    }

    /// Live preview: snap the dragged wire's loose end to the hovered input
    /// port (if any) so the connection target reads from the router hit-test.
    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        let target = over.and_then(|dp| parse_input_port_tag(&dp.tag));
        self.preview
            .set_with(|prev| (*prev).map(|p| Preview { to: target, ..p }));
    }

    /// Commit the drag if it landed on an input port: a reconnect drag (R1174 —
    /// a wired input pulled loose) moves the grabbed edge's target through the
    /// [`reconnect_edge`](Self::reconnect_edge) SSOT; a fresh connect drag mints
    /// a new edge through [`add_edge`](Self::add_edge). R1220 — a fresh connect
    /// released on the empty canvas (tag == [`GRAPH_TAG`], not over a port / node)
    /// instead opens the pin-drop create menu at the drop point
    /// ([`open_pin_create`](Self::open_pin_create)). Any other drop (off a port, on
    /// a node body, outside the window) is a no-op — the original wiring is left
    /// untouched (the connect gesture's "release in empty space cancels" behaviour,
    /// which a *reconnect* still gets since it opens no menu).
    fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        // R1411 — a palette card dropped on the canvas instantiates its node at
        // the drop point. `pending_press` still holds the pressed PALETTE index
        // (reset by `end_gesture` below); the drop graph point is the R1220
        // pin-drop projection of the release fraction over `GRAPH_TAG`. A release
        // NOT over the canvas (still over the palette strip, off-window) adds
        // nothing here — an in-place press-release adds at the spawn point via
        // the PointerUp click path instead, so the two never both fire.
        if payload.kind == PALETTE_DRAG_KIND {
            if let PendingPress::Palette(kind) = self.pending_press.get() {
                if let Some(dp) = over.as_ref().filter(|dp| dp.tag == GRAPH_TAG) {
                    let (gx, gy) = cursor_graph_at(
                        &self.scroll,
                        self.zoom.get(),
                        f64::from(dp.x_rel),
                        f64::from(dp.y_rel),
                    );
                    self.add_node_at(
                        kind,
                        clamp_node_x(round_i32(gx)),
                        clamp_node_y(round_i32(gy)),
                    );
                }
            }
            self.preview.set(None);
            self.end_gesture();
            return;
        }
        let reconnect = self.preview.get().and_then(|p| p.reconnect);
        if let Some((to_node, to_port)) = over.as_ref().and_then(|dp| parse_input_port_tag(&dp.tag))
        {
            if let Some(edge) = reconnect {
                self.reconnect_edge(edge, to_node, to_port);
            } else if let IntrospectValue::Text(src) = &payload.value {
                if let Some((from_node, from_port)) = split_node_port(src) {
                    self.add_edge(from_node, from_port, to_node, to_port);
                }
            }
        } else if reconnect.is_none() {
            // R1220 — a fresh wire dropped on the empty canvas: open the create
            // menu at the drop point. `dp.tag == GRAPH_TAG` (the deepest tag over
            // empty space) → the fraction projects cleanly to a graph point; a
            // release over a node body / outside falls through to a plain cancel.
            if let (Some(dp), IntrospectValue::Text(src)) = (
                over.as_ref().filter(|dp| dp.tag == GRAPH_TAG),
                &payload.value,
            ) {
                if let Some((from_node, from_port)) = split_node_port(src) {
                    let (gx, gy) = cursor_graph_at(
                        &self.scroll,
                        self.zoom.get(),
                        f64::from(dp.x_rel),
                        f64::from(dp.y_rel),
                    );
                    let at_screen = (
                        upx(round_i32(f64::from(dp.x_rel) * f64::from(WIN_W))),
                        upx(round_i32(f64::from(dp.y_rel) * f64::from(WIN_H))),
                    );
                    self.open_pin_create(
                        from_node,
                        from_port,
                        (round_i32(gx), round_i32(gy)),
                        at_screen,
                    );
                }
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

/// R1353 — this binding's declared introspect surface, lifted out of
/// `schema()` so the fn stays under the workspace line cap now that each
/// parametric field declares its argument rather than hand-spelling a
/// template string. Same shape as `pinion_audio::RT_EXTERNAL_FIELDS`.
const NODE_GRAPH_SCHEMA_FIELDS: &[SchemaField] = &[
    SchemaField::new("node_count", "int"),
    SchemaField::new("edge_count", "int"),
    // R1441 — how many edge crossings a fresh layered layout of THIS graph would
    // have. The tidiness metric the layout is judged by, read-only and derived,
    // so an AI can ask "is this graph tangled?" and see `auto_layout` improve it.
    SchemaField::new("layout_crossings", "int"),
    // R1441 — the Brandes-Köpf guarantee as data: how many inner segments a fresh
    // layout has, and how many of those it drew on one coordinate. A bend is not a
    // node, so this is the only way a client can check the promise at all.
    SchemaField::new("layout_inner_segments", "int"),
    SchemaField::new("layout_straight_inner", "int"),
    SchemaField::new("node_ids", "string"),
    SchemaField::new("edge_ids", "string"),
    // R1242 — the reroute-knot discriminator + enumeration.
    SchemaField::new("reroute_ids", "string"),
    SchemaField::parametric(
        "node.<id>.is_reroute",
        "bool",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1256 — the rename-stable compute identity (Add/Multiply/...).
    SchemaField::parametric(
        "node.<id>.op",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1257 — is-authorable-source flag (its `value` is intervene-able).
    SchemaField::parametric(
        "node.<id>.is_source",
        "bool",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1596 — the frame this node sits in, or Null on the bare canvas.
    SchemaField::parametric(
        "node.<id>.parent",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1241 — dissolve-eligibility reads (the twins of the dissolve verb).
    SchemaField::new("dissolvable_ids", "string"),
    SchemaField::parametric(
        "dissolvable.<id>",
        "bool",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1596 — WHICH wires a dissolve would cut, beside whether it cuts any.
    // Empty exactly when `dissolvable` is true, so the boolean and the reason
    // are one derivation read two ways.
    SchemaField::parametric(
        "dissolve_severs.<id>",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1227 — comment-frame introspection: the annotation layer as data.
    SchemaField::new("frame_count", "int"),
    SchemaField::new("frame_ids", "string"),
    SchemaField::parametric(
        "frame.<id>.title",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1596 — containment read from BOTH ends and at BOTH depths: `contains` is one
    // level, `contents` everything below it, and `parent` the frame this frame is inside.
    // The DCC exposes `node.parent` to Python and has no accessor for a frame's children
    // at all.
    SchemaField::parametric(
        "frame.<id>.contents",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "frame.<id>.parent",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "frame.<id>.x",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "frame.<id>.y",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "frame.<id>.w",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "frame.<id>.h",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "frame.<id>.contains",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::new("selected", "int"),
    SchemaField::new("selected_ids", "string"),
    SchemaField::new("selected_edge", "int"),
    SchemaField::new("renaming", "int"),
    SchemaField::new("editing", "json"),
    SchemaField::action("begin_rename", "int"),
    SchemaField::action("begin_edit_default", "string"),
    SchemaField::action("begin_edit_value", "int"),
    SchemaField::action("begin_edit_detail", "string"),
    SchemaField::parametric(
        "node.<id>.title",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.x",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.y",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1441 — the card's laid-out height, so a client can reason about a node's
    // CENTRE (where an edge attaches) instead of its top edge.
    SchemaField::parametric(
        "node.<id>.h",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.inputs",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.outputs",
        "int",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.input_types",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.output_types",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    SchemaField::parametric(
        "node.<id>.input_default.<port>",
        "json",
        const {
            &[
                SchemaArg::open("id", "int"),
                SchemaArg::open("port", "string"),
            ]
        },
    ),
    // R1260 — the debugger read: the value that actually resolves into
    // an input port (wired source coerced, else the default; null on a
    // cycle).
    SchemaField::parametric(
        "node.<id>.resolved_input.<port>",
        "json",
        const {
            &[
                SchemaArg::open("id", "int"),
                SchemaArg::open("port", "string"),
            ]
        },
    ),
    // R1255 — the evaluated output value (a `Float` reads float, a
    // `Vector` a `{hex,r,g,b,a}` object, `null` on a cycle) — hence json.
    SchemaField::parametric(
        "node.<id>.value",
        "json",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R916 — the Details panel's selection-relative addressing: each
    // `detail.<field>` resolves against the *single* selected node (the
    // R909 inspector pattern, now inside the node-graph editor). `Null`
    // when the selection is not exactly one node. R917 — the mirror is
    // *complete*: every readable `node.<id>.<field>` above has a
    // `detail.<field>` twin (the delegation answers any field), so the
    // schema declares the full set, not a subset. `detail.node` is the
    // selected id, read-only (write the selection via `selected` /
    // `selected_ids`); the rest are read + intervene.
    SchemaField::new("detail.node", "int"),
    SchemaField::new("detail.title", "string"),
    SchemaField::new("detail.op", "string"),
    SchemaField::new("detail.is_source", "bool"),
    SchemaField::new("detail.x", "int"),
    SchemaField::new("detail.y", "int"),
    SchemaField::new("detail.inputs", "int"),
    SchemaField::new("detail.outputs", "int"),
    SchemaField::new("detail.input_types", "string"),
    SchemaField::new("detail.output_types", "string"),
    SchemaField::parametric(
        "detail.input_default.<port>",
        "json",
        const { &[SchemaArg::open("port", "string")] },
    ),
    SchemaField::parametric(
        "detail.resolved_input.<port>",
        "json",
        const { &[SchemaArg::open("port", "string")] },
    ),
    SchemaField::new("detail.value", "json"),
    SchemaField::parametric(
        "edge.<id>",
        "string",
        const { &[SchemaArg::open("id", "int")] },
    ),
    // R1255 — the graph-evaluation reads (no write twin — derived).
    SchemaField::new("eval.output", "json"),
    SchemaField::new("eval.acyclic", "bool"),
    // R1260 — the ids of the nodes ON a cycle (CSV), localising a null.
    SchemaField::new("eval.cycle_nodes", "string"),
    SchemaField::new("viewport.x", "float"),
    SchemaField::new("viewport.y", "float"),
    SchemaField::new("viewport.zoom", "float"),
    SchemaField::action("send", "string"),
    SchemaField::action("add_node", "string"),
    SchemaField::action("frame_all", "json"),
    SchemaField::action("add_edge", "string"),
    SchemaField::action("remove_edge", "int"),
    SchemaField::action("reconnect_edge", "string"),
    // R1226 — the wire knife: cut every edge the segment "x1,y1,x2,y2"
    // (graph units) crosses, as one undo step. Returns the CSV of cut
    // edge ids (mirrors `edge_ids`), empty when nothing was crossed.
    SchemaField::action("cut_wires", "string"),
    // R1235 — splice a reroute node into edge `<int>`; returns the new
    // node id (`Null` for an unknown edge).
    SchemaField::action("add_reroute", "int"),
    // R1227 — comment-frame verbs: `add_frame` (no arg) frames the
    // current node selection, returning the new frame id (`Null` when
    // nothing is selected); `remove_frame` deletes a frame by id.
    SchemaField::action("add_frame", "int"),
    SchemaField::action("remove_frame", "int"),
    // R1596 — the DCC's NODE_OT_attach / NODE_OT_detach: put the selection
    // into the frame it is sitting on, or take it out one level. Both answer
    // the CSV of nodes whose frame changed, where the DCC's operators report
    // only whether the operator ran at all. R1598 — swap what a node IS,
    // keeping which node it is.
    SchemaField::action("swap_node", "string"),
    SchemaField::action("attach", "string"),
    SchemaField::action("detach", "string"),
    SchemaField::action("delete_node", "int"),
    SchemaField::action("delete_selected", "json"),
    // R1236 — dissolve = delete + reconnect through a 1-in/1-out node
    // (the reroute inverse). `dissolve_node <id>`; `dissolve_selected`
    // (no arg) dissolves the lone selected node.
    SchemaField::action("dissolve_node", "int"),
    SchemaField::action("dissolve_selected", "json"),
    SchemaField::action("select_all", "json"),
    // R1592 — the DCC's NODE_OT_select_lasso / _circle, in graph units.
    // `select_lasso "x,y;x,y;..."` (three vertices or more, closed by
    // derivation); `select_circle "x,y,r"`. Both answer how many nodes the
    // shape took, and both are the SAME call the marquee makes — a
    // `pinion_core::region::Region` handed to `apply_region`.
    SchemaField::action("select_lasso", "string"),
    SchemaField::action("select_circle", "string"),
    SchemaField::action("nudge", "string"),
    // R948 — align / distribute the selection (no args; the AI-first
    // peer of an editor's align toolbar). Each returns whether the
    // graph changed.
    SchemaField::action("align_left", "json"),
    SchemaField::action("align_center_h", "json"),
    SchemaField::action("align_right", "json"),
    SchemaField::action("align_top", "json"),
    SchemaField::action("align_center_v", "json"),
    SchemaField::action("align_bottom", "json"),
    SchemaField::action("distribute_h", "json"),
    SchemaField::action("distribute_v", "json"),
    // R1383 — tidy the whole graph into a layered left-to-right arrangement
    // (Sugiyama); no args, ONE undo step, returns whether anything moved.
    SchemaField::action("auto_layout", "json"),
    // R1390 — relax the whole graph into a force-directed (organic) cluster;
    // no args, ONE undo step, returns whether anything moved.
    SchemaField::action("force_layout", "json"),
    SchemaField::new("serialized", "string"),
    SchemaField::action("set_graph", "string"),
    SchemaField::action("save", "json"),
    SchemaField::action("load", "json"),
    // R1220 — the pin-drop create menu (drag off a pin → typed menu →
    // auto-wire). `pin_create` reads the open menu (Null when closed);
    // the verbs open / filter / rove / commit / cancel it — the AI-first
    // peer of the live gesture, funnelling through the same coordinator.
    SchemaField::new("pin_create", "json"),
    SchemaField::action("open_pin_create", "string"),
    SchemaField::action("pin_create_filter", "string"),
    SchemaField::action("pin_create_highlight", "string"),
    SchemaField::action("commit_pin_create", "string"),
    SchemaField::action("cancel_pin_create", "json"),
];

impl ExternalIntrospect for NodeGraphExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(NODE_GRAPH_SCHEMA_FIELDS)
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "node_count" => Some(IntrospectValue::Int(int_of(self.node_count()))),
            "edge_count" => Some(IntrospectValue::Int(int_of(edges(&self.graph()).len()))),
            "layout_crossings" => Some(IntrospectValue::Int(int_of(layout_crossings(
                &self.graph(),
            )))),
            "layout_inner_segments" => Some(IntrospectValue::Int(int_of(
                layout_inner_straightness(&self.graph()).0,
            ))),
            "layout_straight_inner" => Some(IntrospectValue::Int(int_of(
                layout_inner_straightness(&self.graph()).1,
            ))),
            // CSV of the *current* (possibly sparse after deletes) stable ids —
            // the enumeration handle an AI needs now that addressing is by id.
            "node_ids" => Some(IntrospectValue::Text(csv_ids(
                kind_nodes(&self.graph()).map(|(n, _)| n.id.0),
            ))),
            "edge_ids" => Some(IntrospectValue::Text(csv_ids(
                edges(&self.graph()).iter().map(|e| e.id.0),
            ))),
            // R1242 — the reroute-knot enumeration (the AI-first "find every
            // reroute" handle a self-hosted editor's cleanup pass needs; the
            // `node.<id>.is_reroute` read is the per-node twin).
            "reroute_ids" => Some(IntrospectValue::Text(csv_ids(
                kind_nodes(&self.graph())
                    .filter(|(n, _)| n.is_reroute())
                    .map(|(n, _)| n.id.0),
            ))),
            // R1227 — the comment-frame enumeration handles (the annotation
            // layer as data, exactly like `node_ids` / `edge_ids`).
            "frame_count" => Some(IntrospectValue::Int(int_of(
                frame_nodes(&self.graph()).count(),
            ))),
            "frame_ids" => Some(IntrospectValue::Text(csv_ids(
                frame_nodes(&self.graph()).map(|f| f.id.0),
            ))),
            // R1241 — the AI-first dissolve-eligibility enumeration: the CSV of
            // node ids a dissolve would not cost anything, so an editor can
            // offer "Dissolve" without probing each node. The `dissolvable.<id>`
            // read is the per-node twin.
            "dissolvable_ids" => Some(IntrospectValue::Text(csv_ids(
                kind_node_vec(&self.graph())
                    .iter()
                    .filter(|n| self.dissolvable(n.id))
                    .map(|n| n.id.0),
            ))),
            // R879 — `selected` answers the *single*-selection question:
            // an Int only when exactly one node is selected (a multi-set
            // has no unambiguous "the" node — read `selected_ids`).
            "selected" => Some(match self.selection.get().node() {
                Some(id) => IntrospectValue::Int(i64::from(id.0)),
                None => IntrospectValue::Null,
            }),
            // R879 — the multi-select read twin: CSV of the selected node
            // ids in id order ("" when no node selection).
            "selected_ids" => Some(IntrospectValue::Text(csv_ids(
                self.selection.get().nodes().iter().map(|id| id.0),
            ))),
            "selected_edge" => Some(match self.selection.get().edge() {
                Some(id) => IntrospectValue::Int(i64::from(id.0)),
                None => IntrospectValue::Null,
            }),
            // R878 — the in-flight inline *rename* target (`Null` when idle or
            // when a non-title edit is in flight); the read twin of `invoke
            // begin_rename`. R901 — a degenerate projection of `editing` over
            // `Title` targets: a port-default edit is honestly not a rename, so
            // this stays `Null` then (read `editing` for the full picture —
            // [[wire-form-read-write-symmetry]], the representation-richer-but-
            // old-contract-preserved discipline).
            "renaming" => Some(match self.editing.get() {
                Some(ActiveEdit {
                    target: EditTarget::Title(id),
                    ..
                }) => IntrospectValue::Int(i64::from(id.0)),
                _ => IntrospectValue::Null,
            }),
            // R901 / R918 — the in-flight inline edit, the honest generalised
            // read: `Null` when idle, else `{ kind, node, port?, surface }` where
            // `kind` is `title` / `port_default` / `pos_x` / `pos_y` and `surface`
            // is `card` / `panel` (the read twin of the `begin_*` invokes + the
            // double-click / panel-row-click entries).
            "editing" => Some(
                self.editing
                    .get()
                    .map_or(IntrospectValue::Null, active_edit_introspect),
            ),
            // R852 — the whole graph as one JSON blob (the AI-first read; its
            // write-twin is `invoke set_graph`).
            "serialized" => Some(IntrospectValue::Text(self.serialized_json())),
            // R1220 — the open pin-drop create menu as JSON (Null when closed):
            // the introspection twin of the painted floating menu.
            "pin_create" => Some(self.pin_create_introspect()),
            // R877 — the viewport, in zoom-independent graph units (pan) +
            // the zoom factor. Write-twins: `intervene viewport.{x,y,zoom}`.
            // R1183 — the pan in graph units is the graph point under the
            // canvas top-left corner: `cursor_graph(0, 0)`, the inverse SSOT
            // (was an inline `offset / zoom`).
            "viewport.x" => Some(IntrospectValue::Float(self.cursor_graph(0.0, 0.0).0)),
            "viewport.y" => Some(IntrospectValue::Float(self.cursor_graph(0.0, 0.0).1)),
            "viewport.zoom" => Some(IntrospectValue::Float(self.zoom.get())),
            // R916 — `detail.node` is the single selected node id (the alias the
            // Details panel addresses against, same answer as `selected`).
            "detail.node" => self.query("selected"),
            // R1242 — the prefixed (`detail.` / `node.` / `edge.` / `dissolvable.`
            // / `frame.`) reads, extracted to keep `query` within the line ceiling.
            _ => self.query_prefixed(path),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // R877 — viewport write-twins. Pan in graph units (`scroll_to` clamps
        // against the world maxima — read the outcome back via the query
        // twin); zoom clamps to [ZOOM_MIN, ZOOM_MAX], anchored at the canvas
        // centre (an RPC client has no cursor). Strictly `Float` — `as_f64`'s
        // deliberate no-Int-coercion contract (R51.155), matching the slots'
        // declared `float` schema type.
        if let Some(axis) = path.strip_prefix("viewport.") {
            let v = value.as_f64().ok_or(InterveneError::TypeMismatch)?;
            let zoom = self.zoom.get();
            match axis {
                "x" => {
                    let oy = self.scroll.offset().1;
                    // R1191 — offset that pins graph `v` at canvas x=0, via the
                    // forward SSOT (the twin of the `viewport.x` query's
                    // `cursor_graph(0,0)` inverse read).
                    let (ox, _) = graph_anchor_offset(v, 0.0, zoom, 0.0, 0.0);
                    self.scroll.scroll_to(round_i32(ox), oy);
                    return Ok(());
                }
                "y" => {
                    let ox = self.scroll.offset().0;
                    let (_, oy) = graph_anchor_offset(0.0, v, zoom, 0.0, 0.0);
                    self.scroll.scroll_to(ox, round_i32(oy));
                    return Ok(());
                }
                "zoom" => {
                    self.set_zoom_centered(v);
                    return Ok(());
                }
                _ => return Err(InterveneError::UnknownPath),
            }
        }
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
        if path == "selected_ids" {
            return self.intervene_selected_ids(&value);
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
        // R916 — `detail.<field>` writes the *single selected* node's property
        // (the Details panel's AI-first edit), routing through the identical
        // `node.<selected>.<field>` funnel the absolute path uses — so a drag, a
        // card edit, an `intervene node.<id>`, and an `intervene detail.<field>`
        // are one mutation path. Rejected (`UnknownPath`) when the selection is
        // not exactly one node — there is no unambiguous "the" node to edit.
        if let Some(field) = path.strip_prefix("detail.") {
            let node_path = self
                .selected_node_path(field)
                .ok_or(InterveneError::UnknownPath)?;
            return self.intervene(&node_path, value);
        }
        // R1227 — the comment-frame writes (extracted to keep `intervene` within
        // the workspace line ceiling).
        if path.starts_with("frame.") {
            return self.intervene_frame(path, value);
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
                // R918 — route through the shared `apply_set_pos` funnel (clamp +
                // coalescable journal), the SAME path the Details panel's PosX/PosY
                // inline editor commits through, so an RPC move and a panel edit are
                // one undoable mutation. `x` then `y` still fold into one undo step.
                let (x, y) = if field == "x" {
                    (coord, node.y)
                } else {
                    (node.x, coord)
                };
                apply_set_pos(&self.handle(), id, x, y);
                Ok(())
            }
            // R878 — the write twin of `query node.<id>.title` ([[wire-form-
            // read-write-symmetry]]; pre-R878 this slot was ReadOnly). Routes
            // through the `apply_rename` SSOT, so an RPC rename journals the
            // same undoable `RenameCmd` an interactive commit does. An
            // empty / whitespace title is a value rejection (`OutOfRange`) —
            // the node keeps its name.
            "title" => match value {
                IntrospectValue::Text(t) => {
                    if apply_rename(&self.handle(), id, &t) {
                        Ok(())
                    } else {
                        Err(InterveneError::out_of_range("a node title cannot be blank"))
                    }
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R898 — port arity and the typed-port lists are read-only (ports
            // are defined by the node kind, edited only by add/remove edges);
            // R1256/R1257 — the compute identity + source flag are
            // construction-time constants.
            "inputs" | "outputs" | "input_types" | "output_types" | "op" | "is_reroute"
            | "is_source" => Err(InterveneError::ReadOnly),
            // R1257 — `value` is the node's output: authored for a SOURCE
            // (write lands its output constant), derived for a compute op / sink
            // (`ReadOnly`). Read via `query node.<id>.value` for every node.
            "value" => self.intervene_source_value(id, value),
            // R899 — set an input port's typed default (the write twin of
            // `query node.<id>.input_default.<port>`); routed through the
            // type-checking [`Self::intervene_input_default`] helper.
            _ => self.intervene_input_default(id, field, value),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(s) => Ok(self.handle_send(&s)),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R849 — create a node by kind name; returns the new node's stable
            // id in one round-trip. An unknown kind is Rejected (the graph is
            // unchanged), the AI-first mirror of a clicked palette card.
            "add_node" => self.invoke_add_node(&args),
            "swap_node" => self.invoke_swap_node(&args),
            "add_edge" => match args {
                IntrospectValue::Text(s) => {
                    let (fnode, fport, tnode, tport) = parse_quad(&s).ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "{path}: malformed argument {s:?} \
                             (expected \"<from_node>.<from_port>,<to_node>.<to_port>\")"
                        ))
                    })?;
                    Ok(IntrospectValue::Bool(
                        self.add_edge(fnode, fport, tnode, tport),
                    ))
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
            // R929 — re-wire an existing edge's target input (the AI-first peer
            // of the live drag-to-reconnect gesture); both funnel to
            // `reconnect_edge`. Arg `"edge,to_node,to_port"`.
            "reconnect_edge" => match args {
                IntrospectValue::Text(s) => {
                    let (edge, tnode, tport) = parse_reconnect(&s).ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "{path}: malformed argument {s:?} \
                             (expected \"<edge>,<to_node>,<to_port>\")"
                        ))
                    })?;
                    Ok(IntrospectValue::Bool(
                        self.reconnect_edge(edge, tnode, tport),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1226 — the wire knife (the AI-first primary path). Arg
            // `"x1,y1,x2,y2"` (graph units); returns the CSV of cut edge ids.
            "cut_wires" => self.invoke_cut_wires(args),
            // R1235 — splice a reroute node into an edge (the AI-first peer of a
            // double-click-on-wire gesture). Arg `<edge_id>`; returns the new id.
            "add_reroute" => self.invoke_add_reroute(&args),
            // R1227 — comment-frame verbs (bodies extracted to keep `invoke`
            // within the workspace line ceiling).
            "add_frame" | "remove_frame" | "attach" | "detach" => self.invoke_frame(path, &args),
            "delete_node" => match args {
                IntrospectValue::Int(i) => {
                    let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.delete_node(NodeId(id))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "delete_selected" => Ok(IntrospectValue::Bool(self.delete_selected())),
            // R1236 — dissolve a node (delete + reconnect A -> B through it), the
            // inverse of `add_reroute`. `dissolve_node <id>` by id; the no-arg
            // `dissolve_selected` (the Alt+Delete gesture) targets the lone
            // selected node.
            "dissolve_node" => self.invoke_dissolve_node(&args),
            "begin_edit_value" => self.invoke_begin_edit_value(&args),
            "dissolve_selected" => Ok(IntrospectValue::Bool(self.dissolve_selected())),
            // R880 — select every node (the keyboard `Ctrl`+`A` twin).
            // `false` on an empty graph.
            "select_all" => Ok(IntrospectValue::Bool(self.select_all())),
            // R1592 — an area selection by a shape the pointer cannot draw
            // over a wire. The DCC needs an operator each; here both are one
            // `Region` value applied by one call.
            "select_lasso" | "select_circle" => self.invoke_region_select(path, &args),
            // R878 — open the inline rename editor: an `Int` targets that
            // node, `Null` targets the selection (the keyboard `F2` twin).
            // `false` on an unknown id / empty selection (graph unchanged).
            "begin_rename" => match args {
                IntrospectValue::Int(i) => {
                    let id = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.begin_rename(NodeId(id))))
                }
                IntrospectValue::Null => {
                    let Some(id) = self.selection.get().node() else {
                        return Ok(IntrospectValue::Bool(false));
                    };
                    Ok(IntrospectValue::Bool(self.begin_rename(id)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R901 — open the inline editor on an input port's default value
            // (the AI-first / test twin of double-clicking a pin's default
            // label). Arg = `"<node>.<port>"`. `false` on an unknown node /
            // out-of-range port (graph unchanged), mirroring `begin_rename`.
            "begin_edit_default" => self.invoke_begin_edit_default(&args),
            // R918 — open the Details panel's inline editor on the selected
            // node's `field` row (the RPC twin of a panel-row click, mirroring
            // `begin_rename` / `begin_edit_default` for the card). The resulting
            // `editing` reads with surface = "panel"; `false` when nothing /
            // multiple nodes are selected or the field key is unknown. The AI's
            // direct edit path is `intervene detail.<field>` — this opens the
            // *inline field* surface symmetrically with the card begins.
            "begin_edit_detail" => match args {
                IntrospectValue::Text(s) => Ok(IntrospectValue::Bool(self.begin_edit_detail(&s))),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R877 — fit the node bbox into the canvas (the keyboard `f`
            // twin). `false` on an empty graph.
            "frame_all" => Ok(IntrospectValue::Bool(self.frame_all())),
            "nudge" => self.invoke_nudge(&args),
            // R852 — replace the graph from a JSON snapshot (the write-twin of
            // `query serialized`); malformed JSON or a version mismatch is
            // Rejected and leaves the graph unchanged.
            "set_graph" => match args {
                IntrospectValue::Text(s) => self
                    .load_json(&s)
                    .then_some(IntrospectValue::Bool(true))
                    .ok_or_else(|| {
                        InvokeError::rejected(
                            "set_graph: the payload is not a graph this editor can load",
                        )
                    }),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R852 — persist to / restore from the Storage backend. `load`
            // returns false (graph unchanged) when nothing is stored yet.
            "save" => Ok(IntrospectValue::Bool(self.save())),
            "load" => Ok(IntrospectValue::Bool(self.load())),
            // R948 / R1220 — align/distribute + pin-drop create verbs dispatch
            // separately (line budget); a non-match falls through to UnknownPath.
            _ => self.invoke_pin_create(path, args).unwrap_or_else(|| {
                match self.invoke_layout(path) {
                    Some(changed) => Ok(IntrospectValue::Bool(changed)),
                    None => Err(InvokeError::UnknownPath),
                }
            }),
        }
    }
}

impl NodeGraphExternal {
    /// R948 — the align / distribute verbs (all no-arg `-> Bool`, the AI-first
    /// peer of an editor's align toolbar), split out of the main `invoke`
    /// match. `false` when the selection is too small or nothing moves; `None`
    /// for a non-layout verb.
    fn invoke_layout(&self, path: &str) -> Option<bool> {
        Some(match path {
            "align_left" => self.align_selected(AlignSpec::Left),
            "align_center_h" => self.align_selected(AlignSpec::CenterH),
            "align_right" => self.align_selected(AlignSpec::Right),
            "align_top" => self.align_selected(AlignSpec::Top),
            "align_center_v" => self.align_selected(AlignSpec::CenterV),
            "align_bottom" => self.align_selected(AlignSpec::Bottom),
            "distribute_h" => self.distribute_selected(DistributeAxis::Horizontal),
            "distribute_v" => self.distribute_selected(DistributeAxis::Vertical),
            // R1383 — whole-graph layered auto-layout (no selection needed).
            "auto_layout" => self.auto_layout(),
            // R1390 — whole-graph force-directed (organic) layout.
            "force_layout" => self.force_layout(),
            _ => return None,
        })
    }

    /// R1220 — the pin-drop create-menu verbs, split out of the main `invoke`
    /// match (keeps it under the line budget, the [`invoke_layout`](Self::invoke_layout)
    /// precedent). `Some(result)` for a create verb, `None` for anything else (so
    /// the caller falls through to `invoke_layout` / `UnknownPath`).
    fn invoke_pin_create(
        &self,
        path: &str,
        args: IntrospectValue,
    ) -> Option<Result<IntrospectValue, InvokeError>> {
        Some(match path {
            // Open the menu for output `"<node>.<port>"` (the AI-first twin of
            // dragging a wire off that pin into empty space). The node lands to
            // the right of its source; `false` (no menu) on an invalid pin or an
            // output nothing can consume.
            "open_pin_create" => match args {
                IntrospectValue::Text(s) => {
                    let Some((from_node, from_port)) = parse_node_port(&s) else {
                        return Some(Err(InvokeError::rejected(format!(
                            "open_pin_create: malformed argument {s:?} \
                             (expected \"<node>.<port>\")"
                        ))));
                    };
                    let Some(src) = self.node_by_id(from_node) else {
                        return Some(Ok(IntrospectValue::Bool(false)));
                    };
                    // R1248 — route the source's right edge through `right()`
                    // (i.e. `width()`), NOT raw `NODE_W`: a reroute knot source
                    // (`invoke open_pin_create "<knot>.0"`) is 18px wide, so a raw
                    // `NODE_W` placed the create menu ~112px off its true edge (the
                    // width() SSOT peer R1243's self-grep missed).
                    let at_graph = (src.right() + PIN_CREATE_GAP, src.y);
                    let (cx, cy) = graph_to_canvas(
                        &self.scroll,
                        self.zoom.get(),
                        f64::from(at_graph.0),
                        f64::from(at_graph.1),
                    );
                    Ok(IntrospectValue::Bool(self.open_pin_create(
                        from_node,
                        from_port,
                        at_graph,
                        (upx(round_i32(cx)), upx(round_i32(cy))),
                    )))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Set the type-to-narrow filter text (the keyboard type-ahead funnels
            // here too). `false` when no menu is open.
            "pin_create_filter" => match args {
                IntrospectValue::Text(s) => Ok(IntrospectValue::Bool(self.set_pin_filter(&s))),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Move the roving highlight by a signed delta (the arrow-key twin;
            // wraps). `false` when no menu / empty filtered list.
            "pin_create_highlight" => match args {
                IntrospectValue::Text(s) => match s.trim().parse::<i32>() {
                    Ok(delta) => Ok(IntrospectValue::Bool(self.move_pin_highlight(delta))),
                    Err(_) => Err(InvokeError::rejected(format!(
                        "pin_create_highlight: {s:?} is not a step delta"
                    ))),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // Commit a create: `Text` names the kind (must be a current
            // candidate), `Null` commits the highlighted item (the Enter twin).
            // Returns the new node's id, or Rejected (menu left open).
            "commit_pin_create" => {
                let committed = match args {
                    IntrospectValue::Text(s) => {
                        match PALETTE.iter().position(|&(name, _)| name == s) {
                            Some(kind) => self.commit_pin_create_kind(kind),
                            None => {
                                return Some(Err(Self::unknown_kind_reason(
                                    "commit_pin_create",
                                    &s,
                                )));
                            }
                        }
                    }
                    IntrospectValue::Null => self.commit_pin_create_highlighted(),
                    _ => return Some(Err(InvokeError::TypeMismatch)),
                };
                committed
                    .map(|id| IntrospectValue::Int(i64::from(id.0)))
                    .ok_or_else(|| {
                        InvokeError::rejected(
                            "commit_pin_create: no pin-create menu is open on a \
                             port this kind could connect to",
                        )
                    })
            }
            // Close the menu without creating (the Escape / click-away twin).
            // `false` when no menu was open.
            "cancel_pin_create" => Ok(IntrospectValue::Bool(self.cancel_pin_create())),
            _ => return None,
        })
    }
}

// ─── keyboard (graph focused) ──────────────────────────────────────

fn nudge_ok(intro: &mut dyn ExternalIntrospect, dx: i32, dy: i32) -> bool {
    matches!(
        intro.invoke("nudge", IntrospectValue::Text(format!("{dx},{dy}"))),
        Ok(IntrospectValue::Bool(true))
    )
}

/// R852 — map a held-`Ctrl` keystroke to a persistence verb on the graph
/// coordinator: `Ctrl+S` saves, `Ctrl+O` opens (loads). `None` otherwise.
fn save_load_verb(key: &str, modifiers: Modifiers) -> Option<&'static str> {
    if !modifiers.command_key() {
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

/// R877 — a held-`Ctrl` zoom keystroke (the browser / IDE convention).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoomKey {
    /// `Ctrl`+`=` / `Ctrl`+`+` — one step in.
    In,
    /// `Ctrl`+`-` — one step out.
    Out,
    /// `Ctrl`+`0` — reset to 100%.
    Reset,
}

fn zoom_verb(key: &str, modifiers: Modifiers) -> Option<ZoomKey> {
    if !modifiers.command_key() {
        return None;
    }
    match key {
        "=" | "+" => Some(ZoomKey::In),
        "-" => Some(ZoomKey::Out),
        "0" => Some(ZoomKey::Reset),
        _ => None,
    }
}

// ─── R878 / R901 inline edit (commit / cancel, owner-scoped) ───────

/// Commit the in-flight inline edit: read the shared field's text and route
/// it to the target's field SSOT through [`apply_edit_commit`] (a title trims
/// and rejects empty, a port default parses by kind; an unchanged or malformed
/// value keeps the prior — no data loss, no spurious undo step). Mirrors
/// `hello-data-grid::commit_edit`.
fn commit_edit(restore_focus: bool) {
    let Some(active) = use_active_edit().get() else {
        return;
    };
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    apply_edit_commit(&use_graph_handle(), active.target, &text);
    end_edit_mode(restore_focus);
}

/// Cancel the in-flight edit — leave the target's value untouched, restore
/// focus.
fn cancel_edit() {
    end_edit_mode(true);
}

/// Shared finish-edit teardown — clear the `editing` flag + wipe the field so
/// the next edit starts from a fresh seed; restore canvas focus on request
/// (the keyboard paths; a blur commit leaves focus where the click landed).
fn end_edit_mode(restore_focus: bool) {
    use_active_edit().set(None);
    use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
    if restore_focus {
        pinion_core::focus_request::request(GRAPH_TAG);
    }
}

/// R878 / R901 — the inline-edit keymap over the shared field: the lifted
/// [`pinion_core::edit_field_keymap`] SSOT (3rd consumer, after the data-grid
/// / property-grid typed editors). The keystroke gate is the *target's*
/// [`CellKind`]: a node title is plain [`CellKind::Text`] (every printable
/// reaches the field), a port default is the port's typed kind (a `Float`
/// accepts digits / sign / `.`, a `Color` hex digits + `#`). Stray named keys
/// defer — inert while the field owns focus.
fn apply_key_edit(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    let kind = use_active_edit()
        .get()
        .map_or(CellKind::Text, |a| edit_target_kind(a.target));
    pinion_core::edit_field_keymap(
        scene,
        EDIT_TF_TAG,
        key,
        modifiers,
        kind,
        || commit_edit(true),
        cancel_edit,
    )
}

/// R901 — the keystroke-gate [`CellKind`] for an in-flight edit target: plain
/// text for a title, the port's typed kind for a port default (falling back to
/// `Text` for a vanished port — the field is about to close on the next
/// repaint either way).
fn edit_target_kind(target: EditTarget) -> CellKind {
    match target {
        EditTarget::Title(_) => CellKind::Text,
        EditTarget::PortDefault { node, port } => {
            port_default_kind(&use_document().get(), node, port).unwrap_or(CellKind::Text)
        }
        // R918 — a position is an integer: the gate accepts digits and a leading
        // sign, the same `CellKind::Int` the data-grid / property-grid use.
        EditTarget::PosX(_) | EditTarget::PosY(_) => CellKind::Int,
        // R1264 — a source constant is gated by its own typed kind (a `Float`
        // source accepts digits / sign / `.`, a `Color` source hex digits + `#`).
        EditTarget::SourceConst(id) => {
            source_const_kind(&use_document().get(), id).unwrap_or(CellKind::Text)
        }
    }
}

/// R901 / R918 — the structured `query editing` read for an in-flight edit:
/// `{ kind, node, port?, surface }`. `kind` is `title` / `port_default` /
/// `pos_x` / `pos_y`; `surface` is `card` / `panel` (R918 — *which* surface
/// hosts the field). The honest generalisation of the R878 `query renaming` int,
/// which survives as a degenerate projection over `Title` targets.
fn active_edit_introspect(active: ActiveEdit) -> IntrospectValue {
    let mut obj = match active.target {
        EditTarget::Title(id) => serde_json::json!({ "kind": "title", "node": id.0 }),
        EditTarget::PortDefault { node, port } => {
            serde_json::json!({ "kind": "port_default", "node": node.0, "port": port })
        }
        EditTarget::PosX(id) => serde_json::json!({ "kind": "pos_x", "node": id.0 }),
        EditTarget::PosY(id) => serde_json::json!({ "kind": "pos_y", "node": id.0 }),
        EditTarget::SourceConst(id) => {
            serde_json::json!({ "kind": "source_value", "node": id.0 })
        }
    };
    obj["surface"] = serde_json::Value::from(match active.surface {
        EditSurface::Card => "card",
        EditSurface::Panel => "panel",
    });
    IntrospectValue::Json(obj)
}

/// R1220 — a single printable (alphanumeric) key, for the create menu's
/// type-to-narrow filter; `None` for a named key ("Escape", "ArrowUp", …) or a
/// multi-char string, so those stay swallowed by the modal menu.
fn single_printable(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_alphanumeric()).then_some(c)
}

/// R1220 — drive the open pin-drop create menu from the keyboard, funnelling
/// through the SAME invoke verbs the RPC path uses (the invoke-funnel discipline
/// — shell keys and AI client share one wire). The menu is modal: Escape cancels,
/// Enter commits the highlighted item, arrows rove, Backspace / a printable char
/// edit the filter, and every other key is swallowed (returns `true`) so a graph
/// shortcut never leaks through an open menu. `menu` is the `query pin_create`
/// JSON (its `filter` seeds the type-ahead edit).
fn pin_create_key(
    intro: &mut dyn ExternalIntrospect,
    menu: &serde_json::Value,
    key: &str,
    modifiers: Modifiers,
) -> bool {
    let mut set_filter = |filter: String| {
        let _ = intro.invoke("pin_create_filter", IntrospectValue::Text(filter));
    };
    let filter_of = || {
        menu.get("filter")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    match key {
        "Escape" => {
            let _ = intro.invoke("cancel_pin_create", IntrospectValue::Null);
        }
        "Enter" => {
            let _ = intro.invoke("commit_pin_create", IntrospectValue::Null);
        }
        "ArrowDown" => {
            let _ = intro.invoke(
                "pin_create_highlight",
                IntrospectValue::Text("1".to_owned()),
            );
        }
        "ArrowUp" => {
            let _ = intro.invoke(
                "pin_create_highlight",
                IntrospectValue::Text("-1".to_owned()),
            );
        }
        "Backspace" => {
            let mut filter = filter_of();
            filter.pop();
            set_filter(filter);
        }
        _ => {
            // R1223 — a real character keypress (no command / Alt chord) types
            // into the type-to-narrow filter; a chord like Ctrl+Z / Ctrl+A is
            // SWALLOWED as a no-op, never appended. The pre-R1223 modal path
            // passed only the bare `key`, so Ctrl+Z typed `z` into the filter —
            // the idle path's `command_key()` gate was missing here.
            if !modifiers.command_key()
                && !modifiers.alt_key()
                && let Some(ch) = single_printable(key)
            {
                let mut filter = filter_of();
                filter.push(ch);
                set_filter(filter);
            }
        }
    }
    true
}

fn apply_key_graph(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    // R1220 — while the pin-drop create menu is open it is modal: menu keys win
    // over every graph shortcut (undo / zoom / nudge / …). Scoped borrow so the
    // `invoke_undo(scene, …)` re-borrow below still type-checks when idle.
    if let Some(node) = scene.find_external_with_tag_mut(GRAPH_TAG) {
        if let Some(intro) = node.handle.introspect_mut() {
            if let Some(IntrospectValue::Json(menu)) = intro.query("pin_create") {
                return pin_create_key(intro, &menu, key, modifiers);
            }
        }
    }
    if let Some(verb) = undo_redo_verb(key, modifiers) {
        return invoke_undo(scene, verb);
    }
    let Some(node) = scene.find_external_with_tag_mut(GRAPH_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    // R877 — zoom keys funnel through the same `viewport.zoom`
    // query/intervene pair the RPC path drives (the invoke-funnel
    // discipline: shell keyboard and AI client share one wire-form).
    // The centre anchor lives behind the intervene arm.
    if let Some(verb) = zoom_verb(key, modifiers) {
        let target = match verb {
            ZoomKey::Reset => 1.0,
            ZoomKey::In | ZoomKey::Out => {
                let Some(IntrospectValue::Float(zoom)) = intro.query("viewport.zoom") else {
                    return false;
                };
                if verb == ZoomKey::In {
                    zoom * ZOOM_STEP
                } else {
                    zoom / ZOOM_STEP
                }
            }
        };
        let _ = intro.intervene("viewport.zoom", IntrospectValue::Float(target));
        return true;
    }
    // R852 — Ctrl+S save / Ctrl+O open, on the graph coordinator itself.
    if let Some(verb) = save_load_verb(key, modifiers) {
        let _ = intro.invoke(verb, IntrospectValue::Null);
        return true;
    }
    // R880 — Ctrl+A selects every node (the editor-canvas convention).
    // While a rename is in flight the focused field's keymap intercepts
    // its own Ctrl+A (select-all *text*) before the graph path runs.
    if modifiers.command_key() && key.eq_ignore_ascii_case("a") {
        return matches!(
            intro.invoke("select_all", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        );
    }
    match key {
        "ArrowUp" => nudge_ok(intro, 0, -NUDGE_STEP),
        "ArrowDown" => nudge_ok(intro, 0, NUDGE_STEP),
        "ArrowLeft" => nudge_ok(intro, -NUDGE_STEP, 0),
        "ArrowRight" => nudge_ok(intro, NUDGE_STEP, 0),
        // R1236 — `Alt`+`Delete` DISSOLVES the selected node (delete + reconnect
        // through it, the reroute inverse); plain `Delete` removes it. Both
        // funnel to the RPC verbs the AI-first path drives.
        "Delete" | "Backspace" => {
            let verb = if modifiers.alt_key() {
                "dissolve_selected"
            } else {
                "delete_selected"
            };
            matches!(
                intro.invoke(verb, IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            )
        }
        "Escape" => {
            let _ = intro.intervene("selected", IntrospectValue::Null);
            true
        }
        // R877 — frame the whole graph (the engine / the DCC `F`).
        "f" => matches!(
            intro.invoke("frame_all", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        ),
        // R878 — rename the selected node (the file-manager F2 idiom),
        // through the same `begin_rename` verb the RPC path drives (the
        // invoke-funnel discipline). `Null` args = "the selection".
        "F2" => matches!(
            intro.invoke("begin_rename", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        ),
        _ => false,
    }
}

// ─── paint ─────────────────────────────────────────────────────────

/// One cubic-bezier edge path between two window-space port centres. An
/// S-curve: control points offset horizontally so wires leave / enter ports
/// level (the canonical node-graph wire shape).
fn view_edge(
    tag: String,
    from: (i32, i32),
    to: (i32, i32),
    color: Color,
    width: u32,
    zoom: f64,
) -> Scene {
    // R877 — the curve is *computed* in graph space (the same
    // `edge_curve` SSOT the hit-test samples) and *projected* per control
    // point: pan + zoom is affine, so scaling the four control points
    // scales the exact same cubic — painted wire and clickable wire stay
    // one curve at every zoom.
    let (c1, c2) = edge_curve(from, to);
    let from = (wpx(from.0, zoom), wpx(from.1, zoom));
    let to = (wpx(to.0, zoom), wpx(to.1, zoom));
    let (c1, c2) = (
        (wpx(c1.0, zoom), wpx(c1.1, zoom)),
        (wpx(c2.0, zoom), wpx(c2.1, zoom)),
    );
    let width = wstroke(width, zoom);
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
    let rect = Rect::new(upx(ox), upx(oy), upx(bw), upx(bh));
    // R1358 — the commands are relative to the node's own rect. Rebase by the
    // rect's origin rather than the raw minimum: `upx` clamps a negative min
    // to 0, and the paint adapter translates by exactly `rect.{x,y}`, so only
    // the clamped value is the true inverse of the placement.
    let (org_x, org_y) = (ipx(rect.x), ipx(rect.y));
    let commands = vec![
        PathCommand::MoveTo(ppt(from.0 - org_x, from.1 - org_y)),
        PathCommand::CurveTo {
            c1: ppt(c1.0 - org_x, c1.1 - org_y),
            c2: ppt(c2.0 - org_x, c2.1 - org_y),
            end: ppt(to.0 - org_x, to.1 - org_y),
        },
    ];
    Scene::Path(
        PathNode::new(
            rect,
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

/// R1230 / R1262 — resolve edge `e` to its `(from output-port, to input-port)`
/// centres via a caller-supplied node `lookup`, dropping an edge whose endpoint
/// node is absent. **The ONE endpoint anchor body** the edge paint
/// ([`view_edges`]), the click hit-test ([`NodeGraphExternal::hit_test_edge`]),
/// and the wire knife ([`NodeGraphExternal::cut_wires`]) all resolve through — a
/// port-anchor change (offset, multi-row ports) lands once, so the drawn /
/// clicked / cut wire can never disagree (the R1226 knife had copied
/// `hit_test_edge`'s body verbatim as a third site). R1262 restored this SSOT
/// after R1261's paint-perf pass inlined a copy: the lookup is now a parameter,
/// so [`edge_endpoints`] (linear `node_ref`, the cold callers) and `view_edges`
/// (a per-paint `BTreeMap` index) share this one body instead of two.
fn edge_endpoints_via<'a>(
    lookup: impl Fn(NodeId) -> Option<&'a Node<MaterialOp>>,
    e: &Edge,
) -> Option<((i32, i32), (i32, i32))> {
    Some((
        output_port_center(lookup(e.from.node)?, uidx(e.from.port)),
        input_port_center(lookup(e.to.node)?, uidx(e.to.port)),
    ))
}

/// R1230 — resolve edge `e`'s endpoint centres by a linear `node_ref` lookup
/// (the [`edge_endpoints_via`] SSOT), for the cold callers (hit-test / knife /
/// reroute) where a per-call O(n) is not a per-frame cost.
fn edge_endpoints(graph: &Graph, e: &Edge) -> Option<((i32, i32), (i32, i32))> {
    edge_endpoints_via(|id| graph.tree(TREE).and_then(|tree| tree.node(id)), e)
}

/// All committed edges, resolved to their port centres. Painted behind the
/// node cards; the selected edge paints thicker in the highlight colour. Each
/// edge is tagged by its stable [`EdgeId`].
fn view_edges(
    graph: &Graph,
    selected_edge: Option<EdgeId>,
    theme: &Theme,
    zoom: f64,
) -> Vec<Scene> {
    let color = theme.resolve(ColorRole::Accent);
    let hot = theme.resolve(ColorRole::OnSurface);
    // R1261 — index the nodes by id ONCE (O(nodes)) so each edge's two endpoint
    // lookups are O(log n), not the linear `node_ref` scan (view_edges was
    // O(edges · nodes)). R1262 — the anchor math still routes through the ONE
    // [`edge_endpoints_via`] body (only the lookup differs), so the drawn wire
    // cannot drift from the hit-test / knife on a future port-anchor change.
    let index: BTreeMap<NodeId, &Node<MaterialOp>> =
        kind_nodes(graph).map(|(n, _)| (n.id, n)).collect();
    edges(graph)
        .iter()
        .filter_map(|e| {
            let (from, to) = edge_endpoints_via(|id| index.get(&id).copied(), e)?;
            let (c, w) = if selected_edge == Some(e.id) {
                (hot, SELECTED_EDGE_W)
            } else {
                (color, EDGE_W)
            };
            Some(view_edge(
                format!("{GRAPH_TAG}#edge_{}", e.id),
                from,
                to,
                c,
                w,
                zoom,
            ))
        })
        .collect()
}

/// One port box (a small rounded square on the card edge). `left` / `top`
/// are node-local graph units, projected here.
fn view_port(tag: String, left: i32, top: i32, color: Color, zoom: f64) -> Scene {
    let size = upx(wpx(PORT_SIZE, zoom).max(2));
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag)
            .with_style(BoxStyle::filled(color).with_corner_radius(size / 2))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(wpx(left, zoom)), upx(wpx(top, zoom)))
                    .with_size(Size::px(size, size)),
            ),
    )
}

/// R899 / R1264 — a typed constant **value label** anchored to a pin row: an
/// *unconnected* input port's literal default (the "pin default", tagged
/// `idefault_<id>_<port>`, R899) OR a **source** node's authored output constant
/// (tagged `oconst_<id>`, R1264). A tagged container so an AI client can observe
/// the painted value through `scene/snapshot` (the value is also the
/// `query node.<id>.input_default.<port>` / `node.<id>.value` read); its `Text`
/// child carries the `CellValue::display` string. Placed just right of the port
/// square, node-local. Double-clicking it opens the shared inline editor
/// ([`view_pin_edit_field`]).
fn view_pin_value_label(tag: String, text: &str, top: i32, theme: &Theme, zoom: f64) -> Scene {
    let label = Scene::Text(TextNode::styled(
        text.to_owned(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(wfont(11, zoom))
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label]).with_tag(tag).with_layout(
            LayoutStyle::new()
                .with_absolute_position(upx(wpx(PORT_SIZE + 4, zoom)), upx(wpx(top, zoom)))
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center),
        ),
    )
}

/// R901 / R1264 — the ONE shared inline field painted over a pin-row value label
/// while it is being edited (the [`view_pin_value_label`] swap, the pin-row twin
/// of the header's title-or-field switch): an input port's default (R901) or a
/// source node's output constant (R1264) both host this same field. Tagged
/// [`EDIT_TF_TAG`] by [`tf_paint::view_field`] — the field owns the hit target
/// and focus while open — and positioned where the static label sat, projected
/// through the zoom like every world coordinate.
fn view_pin_edit_field(edit_field: RootState, top: i32, theme: &Theme, zoom: f64) -> Scene {
    let style = tf_paint::TextFieldStyle {
        field_w: upx(wpx(NODE_W - PORT_SIZE - 8, zoom)),
        field_h: upx(wpx(PORT_SIZE + 4, zoom)),
        field_pad: 3,
        font_size_px: wfont(11, zoom),
        ..tf_paint::TextFieldStyle::m3_filled()
    };
    let field = tf_paint::view_field(
        EDIT_TF_TAG,
        edit_field.0,
        edit_field.1,
        theme,
        &style,
        "value",
    );
    Scene::Container(
        ContainerNode::new(vec![field]).with_layout(
            LayoutStyle::new()
                .with_absolute_position(upx(wpx(PORT_SIZE + 4, zoom)), upx(wpx(top, zoom)))
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center),
        ),
    )
}

/// One node card: a header (title) over its input (left) + output (right)
/// ports, absolutely placed at the node's canvas position. The whole card is
/// one drag target; the ports are deeper hit targets for edge connect.
/// R1243 — the selection border every node shape wears: a 2px accent ring when
/// selected, else a 1px outline. The one home for the node-selection border so
/// the full card ([`view_node`]) and the compact knot ([`view_reroute_knot`])
/// can never draw a selected node differently.
fn node_border(selected: bool, theme: &Theme) -> Border {
    if selected {
        Border::new(theme.resolve(ColorRole::Accent), 2)
    } else {
        Border::new(theme.resolve(ColorRole::Outline), 1)
    }
}

/// R1243 — a reroute knot: a compact circular dot painted in its wire's
/// signature colour, in place of a full node card. A reroute
/// ([`NodeGeometry::is_reroute`]) is a wire-routing passthrough, not a compute op,
/// so it carries no header / port rows / inline editors — the wire passes
/// straight through the dot ([`knot_center`] anchors both ports). Tagged
/// `#node_{id}` like any node, so it selects / drags / dissolves through the
/// same paths; a selected knot gains the accent ring every node uses.
fn view_reroute_knot(node: &Node<MaterialOp>, selected: bool, theme: &Theme, zoom: f64) -> Scene {
    let size = upx(wpx(KNOT_SIZE, zoom).max(4));
    // The knot wears its wire's signature colour (the pin colour-coding
    // convention); a reroute always has exactly one typed port.
    let fill = match node.body {
        NodeBody::Kind(MaterialOp::Reroute(ty)) => ty.color(),
        _ => theme.resolve(ColorRole::Outline),
    };
    let border = node_border(selected, theme);
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(format!("{GRAPH_TAG}#node_{}", node.id))
            .with_style(
                BoxStyle::filled(fill)
                    .with_border(border)
                    // A half-diameter radius rounds the square into a dot.
                    .with_corner_radius(size / 2),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(wpx(node.x, zoom)), upx(wpx(node.y, zoom)))
                    .with_size(Size::px(size, size)),
            ),
    )
}

/// R898 / R1264 — the output-port column of a node card: each output port's
/// colour-coded pin, and — for a **source** node — output port 0's authored
/// output constant, either as a static value label ([`view_pin_value_label`],
/// tagged `oconst_<id>`) or the shared inline field while it is being edited.
/// The constant paints BEFORE its pin so the pin dot draws on top of the label /
/// field edge. A compute op / sink has `output_const == None` (its output is
/// derived), so only the pins paint. Extracted from [`view_node`] to keep it
/// under the line ceiling once the source-const branch landed.
fn view_output_ports(
    graph: &Graph,
    node: &Node<MaterialOp>,
    signature: &Signature<MaterialOp>,
    edit: CardEdit,
    theme: &Theme,
    zoom: f64,
) -> Vec<Scene> {
    let id = node.id;
    let mut out = Vec::new();
    for (j, port) in signature.outputs.iter().enumerate() {
        // R1599 — this taxonomy declares no control port, so `value_type` is
        // always answered here; skipping rather than unwrapping is what keeps
        // that a fact about the taxonomy instead of an assumption in the paint.
        let Some(&ty) = port.value_type() else {
            continue;
        };
        if j == 0 {
            if let Some(val) = source_const(graph, id) {
                if edit.target == Some(EditTarget::SourceConst(id)) {
                    out.push(view_pin_edit_field(
                        edit.field,
                        port_row_top(j),
                        theme,
                        zoom,
                    ));
                } else {
                    out.push(view_pin_value_label(
                        format!("{GRAPH_TAG}#oconst_{id}"),
                        &val.display(),
                        port_row_top(j),
                        theme,
                        zoom,
                    ));
                }
            }
        }
        out.push(view_port(
            format!("{GRAPH_TAG}#oport_{id}_{j}"),
            NODE_W - PORT_SIZE,
            port_row_top(j),
            ty.color(),
            zoom,
        ));
    }
    out
}

fn view_node(
    graph: &Graph,
    node: &Node<MaterialOp>,
    selected: bool,
    edit: CardEdit,
    wired_inputs: &BTreeSet<usize>,
    theme: &Theme,
    zoom: f64,
) -> Scene {
    // R1243 — a reroute paints as a compact knot (a wire passthrough), not a
    // card: no header, port rows, or inline editors. It still selects / drags
    // via its `#node_{id}` tag like any node.
    if node.is_reroute() {
        return view_reroute_knot(node, selected, theme, zoom);
    }
    let id = node.id;
    // R1596 — the ports come from the document's own signature derivation, the
    // one `connect` and the evaluator read, rather than from a port list stored
    // on the node. A card cannot paint a port the model does not have.
    let signature = signature_of(graph, id).unwrap_or_else(|| Signature {
        inputs: Vec::new(),
        outputs: Vec::new(),
    });
    // R878 — while this node is being renamed, the header swaps its title
    // text for the ONE shared inline rename field (the data-grid
    // title-or-field switch), sized to the header and projected through the
    // zoom like every other world coordinate.
    let head_inner = if edit.target == Some(EditTarget::Title(id)) {
        let style = tf_paint::TextFieldStyle {
            field_w: upx(wpx(NODE_W - 8, zoom)),
            field_h: upx(wpx(HEADER_H - 6, zoom)),
            field_pad: 4,
            font_size_px: wfont(NODE_TITLE_PX, zoom),
            ..tf_paint::TextFieldStyle::m3_filled()
        };
        tf_paint::view_field(
            EDIT_TF_TAG,
            edit.field.0,
            edit.field.1,
            theme,
            &style,
            "Rename node",
        )
    } else {
        Scene::Text(TextNode::styled(
            node.title(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(wfont(NODE_TITLE_PX, zoom))
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))
    };
    let header = Scene::Container(
        ContainerNode::new(vec![head_inner])
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHighest),
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, 0)
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(upx(wpx(NODE_W, zoom)), upx(wpx(HEADER_H, zoom)))),
            ),
    );

    let mut children = vec![header];
    // R898 — each port paints in its [`PortType`] signature colour, so a
    // connection's validity (output colour vs input colour) is legible at a
    // glance — the engine/the DCC colour-coded-pin convention.
    for (i, port) in signature.inputs.iter().enumerate() {
        children.push(view_port(
            format!("{GRAPH_TAG}#iport_{id}_{i}"),
            0,
            port_row_top(i),
            port.value_type()
                .map_or(PortType::CONTROL_INK, |t| t.color()),
            zoom,
        ));
        // R899 — an *unconnected* input port shows its literal default value
        // (the "pin default"); a wired port draws no label (the incoming edge
        // supplies the value). The default is retained while wired — wiring
        // only hides the label.
        if !wired_inputs.contains(&i) {
            // R901 — while this port's default is being edited, the pin swaps
            // its static value label for the ONE shared inline field (the
            // header's title-or-field switch, applied to the pin default).
            if edit.target == Some(EditTarget::PortDefault { node: id, port: i }) {
                children.push(view_pin_edit_field(
                    edit.field,
                    port_row_top(i),
                    theme,
                    zoom,
                ));
            } else if let Some(val) = input_default(graph, id, i) {
                children.push(view_pin_value_label(
                    format!("{GRAPH_TAG}#idefault_{id}_{i}"),
                    &val.display(),
                    port_row_top(i),
                    theme,
                    zoom,
                ));
            }
        }
    }
    children.extend(view_output_ports(
        graph, node, &signature, edit, theme, zoom,
    ));

    let border = node_border(selected, theme);
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("{GRAPH_TAG}#node_{id}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(border))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(wpx(node.x, zoom)), upx(wpx(node.y, zoom)))
                    // R1248 — width via `node.width()` (the twin of the adjacent
                    // `node.height()`), not literal `NODE_W`: behaviour-equivalent
                    // here (a reroute early-returns to `view_reroute_knot`), but it
                    // keeps the "route through width()" symmetry the R1243 SSOT set.
                    .with_size(Size::px(
                        upx(wpx(node.width(), zoom)),
                        upx(wpx(node.height(), zoom)),
                    )),
            ),
    )
}

/// R880 — the marquee rubber band: a translucent accent fill with a 1px
/// accent border over the swept graph-space rect, projected through the
/// zoom like every world coordinate. Pointer-transparent (it tracks the
/// cursor, so it must never become the hit target under it — the R705
/// decorative-overlay substrate) and tagged, so an AI client can observe
/// the in-flight gesture through `scene/snapshot` (the `#preview` wire
/// precedent).
fn view_marquee(rect: MarqueeRect, theme: &Theme, zoom: f64) -> Scene {
    let (x0, y0, x1, y1) = rect;
    let accent = theme.resolve(ColorRole::Accent);
    let fill = Color {
        a: MARQUEE_FILL_ALPHA,
        ..accent
    };
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(format!("{GRAPH_TAG}#marquee"))
            .with_style(BoxStyle::filled(fill).with_border(Border::new(accent, 1)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(upx(wpx(x0, zoom)), upx(wpx(y0, zoom)))
                    .with_size(Size::px(
                        upx(wpx(x1 - x0, zoom).max(1)),
                        upx(wpx(y1 - y0, zoom).max(1)),
                    ))
                    .with_pointer_transparent(true),
            ),
    )
}

/// R1227 — one a11y `group` per comment frame, named by its title + the count
/// of nodes it contains (the annotation reachable to AT, its membership
/// announced — the same [`frame_members`] relation the `contains` query answers
/// from). Extracted from `access_node` (line ceiling).
///
/// R1596 — the count is the FACT, so what an assistive technology hears and what
/// the wire reports are one derivation; before this round each re-ran the
/// rectangle test and could disagree with a paint mid-drag.
fn frame_access_nodes(graph: &Graph) -> Vec<AccessNode> {
    frame_nodes(graph)
        .map(|f| {
            let members = graph.members(TREE, f.id).len();
            AccessNode::new(format!("{GRAPH_TAG}#frame_{}", f.id), AriaRole::Group)
                .with_name(format!("Comment frame: {} ({members} nodes)", f.title()))
        })
        .collect()
}

/// R1227 — the comment-frame layer: one translucent titled rect per frame,
/// painted BEHIND the edges + nodes (first in `world_children`). The rect is a
/// [`FRAME_FILL_ALPHA`] wash of [`FRAME_COLOR`] with an opaque border + a title
/// in the header strip. Pointer-transparent so a click passes straight through
/// to the nodes / canvas beneath — a frame annotates a region without blocking
/// interaction with the nodes it contains. R1234 made the rect writable over the
/// §5.12 plane (`intervene frame.<id>.{x,y}` moves the frame + contents,
/// `{w,h}` resizes); a GUI grab-to-move handle stays deferred pending a live
/// pointer consumer. Tagged `node_graph#frame_<id>` so that gesture (and the
/// a11y bounds) resolve to the painted rect.
fn view_frames(graph: &Graph, zoom: f64) -> Vec<Scene> {
    let fill = Color {
        a: FRAME_FILL_ALPHA,
        ..FRAME_COLOR
    };
    frame_nodes(graph)
        .map(|f| {
            let title = Scene::Text(
                TextNode::styled(
                    f.title(),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(wfont(NODE_TITLE_PX, zoom))
                        .with_fg(FRAME_COLOR),
                )
                .with_layout(LayoutStyle::new().with_padding(Rect::new(6, 4, 6, 4))),
            );
            Scene::Container(
                ContainerNode::new(vec![title])
                    .with_tag(format!("{GRAPH_TAG}#frame_{}", f.id))
                    .with_style(
                        BoxStyle::filled(fill)
                            .with_border(Border::new(FRAME_COLOR, wstroke(1, zoom)))
                            .with_corner_radius(6),
                    )
                    .with_layout(
                        LayoutStyle::new()
                            .with_absolute_position(upx(wpx(f.x, zoom)), upx(wpx(f.y, zoom)))
                            .with_size(Size::px(
                                upx(wpx(f.width(), zoom).max(1)),
                                upx(wpx(f.height(), zoom).max(1)),
                            ))
                            .with_pointer_transparent(true),
                    ),
            )
        })
        .collect()
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
            TextStyle::new()
                .with_size_px(NODE_TITLE_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_padding(Rect::new(12, 12, 12, 4))),
    ));
    for (idx, &(title, op)) in PALETTE.iter().enumerate() {
        let label = Scene::Text(TextNode::styled(
            format!("{title} ({}/{})", op.inputs().len(), op.outputs().len()),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ));
        items.push(Scene::Container(
            ContainerNode::new(vec![label])
                .with_tag(format!("{GRAPH_TAG}#palette_{idx}"))
                .with_style(BoxStyle::filled(
                    theme.resolve(ColorRole::SurfaceContainerHigh),
                ))
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
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerLow),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(6)
                    .with_size(Size::px(PALETTE_W, WIN_H)),
            ),
    )
}

/// R1220 — the floating pin-drop create menu: a search header (source type +
/// the current type-to-narrow filter) over the type-compatible candidate cards,
/// each a `node_graph#create_<idx>` composite (a click commits that kind through
/// the auto-wire path). The highlighted (roving) card carries an [`ColorRole::Accent`]
/// border — the selected-node idiom. Positioned at the drop point in canvas px
/// (absolute, OUTSIDE the world scroll, like the title / status chrome), so it
/// stays put while it is open. `None` when the source pin has gone invalid (an
/// undo removed its node), matching the coordinator's own re-derivation gate.
fn view_pin_create_menu(pc: &PinCreate, graph: &Graph, theme: &Theme) -> Option<Scene> {
    let from_ty = output_type(graph, pc.from_node, pc.from_port)?;
    let candidates = pin_create_candidates(from_ty, &pc.filter);
    let mut items: Vec<Scene> = Vec::with_capacity(candidates.len() + 1);
    let header = if pc.filter.is_empty() {
        format!("{} {PIN_MENU_ARROW}", from_ty.name())
    } else {
        format!("{} {PIN_MENU_ARROW} {}", from_ty.name(), pc.filter)
    };
    items.push(Scene::Text(
        TextNode::styled(
            header,
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_padding(Rect::new(10, 8, 10, 4))),
    ));
    if candidates.is_empty() {
        items.push(Scene::Text(
            TextNode::styled(
                "no match",
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_layout(LayoutStyle::new().with_padding(Rect::new(10, 4, 10, 8))),
        ));
    }
    for (row, &kind) in candidates.iter().enumerate() {
        let (title, op) = PALETTE[kind];
        let label = Scene::Text(TextNode::styled(
            format!("{title} ({}/{})", op.inputs().len(), op.outputs().len()),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ));
        let mut style = BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh));
        if row == pc.highlight {
            style = style.with_border(Border::new(theme.resolve(ColorRole::Accent), 2));
        }
        items.push(Scene::Container(
            ContainerNode::new(vec![label])
                .with_tag(format!("{GRAPH_TAG}#create_{kind}"))
                .with_style(style)
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(PIN_CREATE_MENU_W - 8, 26))
                        .with_padding(Rect::new(10, 0, 8, 0)),
                ),
        ));
    }
    // Fixed height from the row count (a flex column clips an over-tall child,
    // so the container must span its items): header + one row per candidate
    // (min one for the "no match" line) + top/bottom padding.
    let rows = u32::try_from(candidates.len().max(1)).unwrap_or(1);
    let menu_h = 30 + rows * 30;
    Some(Scene::Container(
        ContainerNode::new(items)
            .with_tag(format!("{GRAPH_TAG}#{PIN_MENU_SUB}"))
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(2)
                    .with_size(Size::px(PIN_CREATE_MENU_W, menu_h))
                    .with_absolute_position(pc.at_screen.0, pc.at_screen.1),
            ),
    ))
}

/// R916 / R918 — one Details-panel property row: a left-aligned `label` over the
/// property's current `value`. Tagged `node_graph#detail_<key>` so a click routes
/// to the coordinator (the palette precedent — a view sibling carrying the
/// primary's prefix), which opens the inline editor on the selected node's
/// matching property. While *this* row hosts the in-flight panel edit, `editing`
/// is `Some(field_state)` and the value display swaps for the ONE shared inline
/// field ([`EDIT_TF_TAG`]) — the card's title-or-field switch, applied to a panel
/// row. A non-edited row stays a read reflection of the model (edits also arrive
/// via the node card, a drag, or `intervene detail.<field>`, and the reactive
/// view re-reflects them).
fn detail_row(
    key: &str,
    label: &str,
    value: &str,
    editing: Option<RootState>,
    theme: &Theme,
) -> Scene {
    let name = Scene::Text(TextNode::styled(
        label.to_owned(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let val = match editing {
        Some(field) => {
            let style = tf_paint::TextFieldStyle {
                field_w: DETAIL_W - 16,
                field_h: 22,
                field_pad: 4,
                font_size_px: 13,
                ..tf_paint::TextFieldStyle::m3_filled()
            };
            tf_paint::view_field(EDIT_TF_TAG, field.0, field.1, theme, &style, label)
        }
        None => Scene::Text(TextNode::styled(
            value.to_owned(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )),
    };
    Scene::Container(
        ContainerNode::new(vec![name, val])
            .with_tag(format!("{GRAPH_TAG}#detail_{key}"))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(2)
                    .with_size(Size::width_px(DETAIL_W - 16))
                    .with_padding(Rect::new(10, 4, 8, 4)),
            ),
    )
}

/// R918 — one Details-panel property row's spec: its `node_graph#detail_<key>`
/// routing key, display `label`, current `value`, and the [`EditTarget`] a click
/// edits. The SSOT [`detail_rows`] enumerates, consumed by both the panel paint
/// ([`view_details_panel`]) and the panel a11y ([`details_access_nodes`]).
struct DetailRow {
    key: String,
    label: String,
    value: String,
    target: EditTarget,
}

/// R918 — the Details panel's property rows for a selected `node`: title,
/// position x / y, then one row per input port's default. The single source the
/// panel paint and a11y both enumerate, so a row's *data* — its key (hence tag),
/// label, value, and edit target — is computed once and cannot drift between the
/// painted row and its AccessNode (the R886.1 paint==a11y discipline). Each
/// consumer still composes its own *presentation* of that shared data (the panel
/// paints `label` and `value` as two cells; the a11y joins them into the node
/// name) — presentation, not row identity, so the byte-matched tag still binds.
fn detail_rows(graph: &Graph, node: &Node<MaterialOp>) -> Vec<DetailRow> {
    let id = node.id;
    let mut rows = vec![
        DetailRow {
            key: "title".to_owned(),
            label: "Title".to_owned(),
            value: node.title(),
            target: EditTarget::Title(id),
        },
        DetailRow {
            key: "x".to_owned(),
            label: "Position X".to_owned(),
            value: node.x.to_string(),
            target: EditTarget::PosX(id),
        },
        DetailRow {
            key: "y".to_owned(),
            label: "Position Y".to_owned(),
            value: node.y.to_string(),
            target: EditTarget::PosY(id),
        },
    ];
    let inputs = signature_of(graph, id)
        .map(|s| s.inputs)
        .unwrap_or_default();
    for (port, declared) in inputs.iter().enumerate() {
        rows.push(DetailRow {
            key: format!("in_{port}"),
            label: format!(
                "In {port} · {}",
                declared.value_type().map_or(CONTROL_PORT, |t| t.name())
            ),
            value: input_default(graph, id, port).map_or_else(String::new, |v| v.display()),
            target: EditTarget::PortDefault { node: id, port },
        });
    }
    rows
}

/// R916 — the Details panel: the single selected node's editable properties
/// (title / position / per-port defaults) reflected as rows, the engine
/// Details "select → inspect" surface. Mirrors the palette sidebar's shape (a
/// sibling column of the canvas). When the selection is not exactly one node
/// it shows a placeholder — there is no unambiguous "the" node to inspect (the
/// `selected` / `detail.node` `Null` case made visible).
fn view_details_panel(
    graph: &Graph,
    selection: &Selection,
    active: Option<ActiveEdit>,
    edit_field: RootState,
    theme: &Theme,
) -> Scene {
    // R918 — the target (if any) the panel is editing: a `Panel`-surface edit
    // whose target's row swaps its value display for the shared field.
    let panel_edit = active
        .filter(|a| a.surface == EditSurface::Panel)
        .map(|a| a.target);
    let row_field = |target: EditTarget| (panel_edit == Some(target)).then_some(edit_field);
    let mut items: Vec<Scene> = vec![Scene::Text(
        TextNode::styled(
            "Details",
            Rect::default(),
            TextStyle::new()
                .with_size_px(NODE_TITLE_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_padding(Rect::new(12, 12, 12, 4))),
    )];

    if let Some((node, _)) = selection.node().and_then(|id| kind_node(graph, id)) {
        for row in detail_rows(graph, node) {
            items.push(detail_row(
                &row.key,
                &row.label,
                &row.value,
                row_field(row.target),
                theme,
            ));
        }
    } else {
        let placeholder = if selection.nodes().len() > 1 {
            "Multiple nodes selected"
        } else {
            "No node selected"
        };
        items.push(Scene::Text(
            TextNode::styled(
                placeholder.to_owned(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(12)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_layout(LayoutStyle::new().with_padding(Rect::new(12, 4, 12, 4))),
        ));
    }

    Scene::Container(
        ContainerNode::new(items)
            .with_tag(DETAIL_TAG)
            .with_aria_label("Details")
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerLow),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(4)
                    .with_size(Size::px(DETAIL_W, WIN_H)),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
/// R878 — the cached paint posture: the shared inline edit field's interaction
/// state + caret byte (the data-grid `RootState` shape).
type RootState = (TextFieldState, u32);

/// R1596 — the shared inline field as ONE card sees it.
///
/// `target` is the edit in flight **on this card** (`None` once it belongs to
/// another card — R901: only the hosting card paints the field), and `field` is
/// the shared posture every card would paint it with. They travel together
/// because neither is meaningful alone: a posture with no target paints nothing,
/// and a target with no posture cannot be drawn.
#[derive(Clone, Copy)]
struct CardEdit {
    target: Option<EditTarget>,
    field: RootState,
}

/// R899 — the node cards, one per node. Each node's wired input set (the ports
/// whose default label is hidden because an edge supplies their value — the
/// single source the paint and the `add_edge` open-port rule share) lowers to a
/// [`view_node`] card. R1261 — the wired-port sets are precomputed ONCE from the
/// edge list (O(edges)) into a per-node map, so the card build is a lookup, not
/// the per-node O(edges) rescan it was (the paint was O(nodes · edges), a
/// frame-time cost the self-hosted editor cannot afford at large-graph scale).
fn view_node_cards(
    graph: &Graph,
    selected: &BTreeSet<NodeId>,
    active: Option<ActiveEdit>,
    edit_field: RootState,
    theme: &Theme,
    zoom: f64,
) -> Vec<Scene> {
    // R918 — only a *card*-surface edit paints over a node card; a panel-surface
    // edit hosts the field in the Details panel instead.
    let card_edit_target = active
        .filter(|a| a.surface == EditSurface::Card)
        .map(|a| a.target);
    let mut wired_by_node: BTreeMap<NodeId, BTreeSet<usize>> = BTreeMap::new();
    for e in edges(graph) {
        wired_by_node
            .entry(e.to.node)
            .or_default()
            .insert(uidx(e.to.port));
    }
    let no_wires = BTreeSet::new();
    kind_nodes(graph)
        .map(|(node, _)| {
            let wired_inputs = wired_by_node.get(&node.id).unwrap_or(&no_wires);
            view_node(
                graph,
                node,
                selected.contains(&node.id),
                CardEdit {
                    target: card_edit_target.filter(|t| t.node() == node.id),
                    field: edit_field,
                },
                wired_inputs,
                theme,
                zoom,
            )
        })
        .collect()
}

// The `&Frame` mirrors the `WidgetCore::view` trait signature (the data-grid
// free-view idiom).
// R1026 — rustfmt's reflow pushed this example view past too_many_lines (100).
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_lines)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let graph = use_document().get();
    let selection = use_selection().get();
    let selected = selection.nodes();
    let selected_edge = selection.edge();
    let preview = use_preview().get();
    // R878 / R901 / R918 — the in-flight inline edit: the shared field paints
    // over a node card's title / pin default (surface = Card) or a Details panel
    // row (surface = Panel), per the active edit's surface.
    let active = use_active_edit().get();
    // R877 — the viewport: zoom projects every world coordinate below;
    // pan is the scroll offset (reading the zoom Signal subscribes the
    // paint Effect, so a wheel zoom repaints reactively; the offset is
    // the ScrollNode's own substrate concern).
    let zoom = use_zoom().get();
    let canvas_scroll = use_canvas_scroll();
    // R1182 — register the edge-drag auto-pan driver on the animation clock
    // (idempotent register-once; ticks only while a node drag hugs the rim).
    use_autopan();

    let mut world_children: Vec<Scene> = Vec::new();

    // R1227 — comment frames sit BEHIND everything (a labelled backdrop wash).
    world_children.extend(view_frames(&graph, zoom));

    // Edges (behind) → preview wire → node cards (on top).
    world_children.extend(view_edges(&graph, selected_edge, &theme, zoom));

    if let Some(p) = preview {
        if let Some((from_node, _)) = kind_node(&graph, p.from_node) {
            let from = output_port_center(from_node, p.from_port);
            let to =
                p.to.and_then(|(tn, tp)| Some(input_port_center(kind_node(&graph, tn)?.0, tp)))
                    .unwrap_or(from);
            world_children.push(view_edge(
                format!("{GRAPH_TAG}#preview"),
                from,
                to,
                theme.resolve(ColorRole::OnSurfaceMuted),
                PREVIEW_W,
                zoom,
            ));
        }
    }

    world_children.extend(view_node_cards(
        &graph, &selected, active, state, &theme, zoom,
    ));

    // R880 — the live marquee rubber band layers over everything in the
    // world (reading the Signal subscribes the paint Effect, so each drag
    // step repaints the band).
    if let Some(rect) = use_marquee_rect().get() {
        world_children.push(view_marquee(rect, &theme, zoom));
    }

    // R877 — the world surface: a fixed WORLD×WORLD extent (scaled by the
    // zoom) the nodes live on, panned by a `ScrollAxis::Both` scroll. The
    // declared size survives the measuring pass (the R877 layout rule for
    // explicitly-sized scroll content), and `update_scroll_state_bounds`
    // derives the pan maxima from it each frame.
    let world_extent = upx(wpx(WORLD, zoom));
    let world = Scene::Container(
        ContainerNode::new(world_children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(world_extent, world_extent))),
    );
    let world_scroll = Scene::Scroll(
        ScrollNode::from_state(canvas_scroll, Rect::new(0, 0, WIN_W, WIN_H), world)
            .with_axis(ScrollAxis::Both),
    );

    // Chrome (title + status) sits OUTSIDE the scroll so it never pans away.
    let mut children = vec![world_scroll];
    children.push(Scene::Text(
        TextNode::styled(
            "Node graph",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, 12)),
    ));

    let sel_label = if selected.len() > 1 {
        format!("{} nodes", selected.len())
    } else if let Some(id) = selected.first() {
        format!(
            "node {}",
            kind_node(&graph, *id).map_or_else(|| "—".to_owned(), |(n, _)| n.title())
        )
    } else if let Some(e) = selected_edge {
        format!("edge {e}")
    } else {
        "none".to_owned()
    };
    // R851 — surface the next-undo action so the history is a visible witness
    // (and reading it subscribes the view to the undo stack's revision signal,
    // so an undo/redo that only moves the cursor still repaints the status).
    let undo_label = use_undo().undo_label();
    // R877 — surface the zoom so the viewport state is a visible witness.
    let status = format!(
        "{} nodes · {} edges · selected: {sel_label} · zoom {}% · undo: {}",
        kind_nodes(&graph).count(),
        edges(&graph).len(),
        round_i32(zoom * 100.0),
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
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(16, i32::try_from(WIN_H).map_or(0, |h| upx(h - 26))),
        ),
    ));

    // R1220 — the pin-drop create menu floats topmost over the canvas (last
    // child = on top), OUTSIDE the world scroll so it stays put. Reading the
    // Signal subscribes the paint Effect, so open / filter / rove repaints.
    if let Some(pc) = use_pin_create().get() {
        if let Some(menu) = view_pin_create_menu(&pc, &graph, &theme) {
            children.push(menu);
        }
    }

    let canvas = Scene::Container(
        ContainerNode::new(children)
            .with_tag(GRAPH_TAG)
            .with_aria_label("Node graph")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_focusable(true),
            ),
    );
    // R849 — palette sidebar beside the canvas. R916 — Details panel sidebar on
    // the right. The canvas keeps its `WIN_W × WIN_H` coordinate system (its rect
    // is merely offset by the palette width; `capture_normalize` resolves against
    // that offset rect), so none of the node geometry / drag math changes — both
    // sidebars are siblings outside `GRAPH_TAG`.
    Scene::Container(
        ContainerNode::new(vec![
            view_palette(&theme),
            canvas,
            view_details_panel(&graph, &selection, active, state, &theme),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_size(Size::px(TOTAL_W, WIN_H)),
        ),
    )
}

// ─── WidgetCore impl ───────────────────────────────────────────────

struct NodeEditorView;

impl WidgetCore for NodeEditorView {
    /// R878 — the cached paint posture: the shared inline edit field's
    /// interaction state + caret byte (the data-grid `RootState` shape).
    /// Everything else the view reads is reactive (`Signal`s subscribe the
    /// paint Effect directly).
    type State = RootState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(NodeGraphExternal::new(
            use_document(),
            use_selection(),
            use_preview(),
            GraphServices {
                undo: use_undo(),
                storage: use_graph_storage(),
                zoom: use_zoom(),
                scroll: use_canvas_scroll(),
                editing: use_active_edit(),
                edit_buffer: use_text_edit_state(EDIT_TF_TAG),
                marquee_rect: use_marquee_rect(),
                node_drag: use_node_drag(),
                pin_create: use_pin_create(),
            },
        ))
    }

    /// R851 — the AI-first undo-history surface. The [`UndoStackExternal`] wraps
    /// the **same** shared [`UndoStack`] the coordinator records onto (via
    /// [`use_undo`]), so `query`/`invoke` at `/node_undo/external/…` observe and
    /// drive the identical history the canvas + keyboard use (one SSOT). It is a
    /// coordinator-only extra: it paints nothing and is not a focus stop.
    ///
    /// R878 / R901 — plus the shared inline edit field (`EDIT_TF_TAG`), a
    /// `TextFieldExternal` modal member (the R790 todomvc `EDIT_TF`
    /// shape): one field reused for every node title AND every input port
    /// default, painted only while an edit is in flight, raising the R793
    /// commit-on-blur intent on a click-away.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(UNDO_TAG, Box::new(UndoStackExternal::new(use_undo()))),
            // R1250 — the shared commit-on-blur inline editor (lifted SSOT).
            blur_committing_field_extra(EDIT_TF_TAG),
        ]
    }

    fn tag() -> &'static str {
        GRAPH_TAG
    }

    fn read_state(scene: &Scene) -> RootState {
        tf_paint::read_text_field_state(scene, EDIT_TF_TAG)
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
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

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        match focused {
            Some(GRAPH_TAG) => apply_key_graph(scene, key, modifiers),
            Some(EDIT_TF_TAG) => apply_key_edit(scene, key, modifiers),
            _ => false,
        }
    }

    /// R878 — route IME composition to the inline edit field while it owns
    /// focus, through the lifted R764.1 SSOT.
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(EDIT_TF_TAG) {
            return false;
        }
        tf_paint::forward_composition_to_field(scene, EDIT_TF_TAG, event)
    }

    /// R878 / R901 / R793 §5.38 — commit-on-blur: the inline field lost focus
    /// (a click elsewhere) while an edit (title or port default) was in flight
    /// → commit without restoring focus. The `editing` gate makes the
    /// post-commit blur a no-op (the data-grid `update` arm verbatim).
    fn update(_state: RootState, intent: &pinion_core::Intent) -> Vec<Command> {
        if intent.tag_str() == EDIT_TF_BLUR_INTENT_TAG && use_active_edit().get().is_some() {
            commit_edit(false);
        }
        Vec::new()
    }
}

/// R918 — the Details panel a11y subtree: a `group` named "Details" whose
/// children are the selected node's property rows (each named "label: value",
/// tagged `node_graph#detail_<key>` to byte-match the painted row — R886.1).
/// While a panel-surface edit is in flight the edited row hosts the shared inline
/// field's textbox child ([`EDIT_TF_TAG`]) — the same paint==a11y one-gate the
/// card uses (R873 / R901.1), so the AT tree advertises the editor exactly when
/// and where it paints. Group-only (no rows) when the selection is not a single
/// node — the panel shows only its placeholder then.
fn details_access_nodes(
    graph: &Graph,
    selection: &Selection,
    active: Option<ActiveEdit>,
    field_state: TextFieldState,
    focused: Option<&str>,
) -> Vec<AccessNode> {
    let mut group = AccessNode::new(DETAIL_TAG, AriaRole::Group).with_name("Details");
    let panel_edit = active
        .filter(|a| a.surface == EditSurface::Panel)
        .map(|a| a.target);
    let mut rows_out: Vec<AccessNode> = Vec::new();
    let mut editor: Option<AccessNode> = None;
    if let Some((node, _)) = selection.node().and_then(|id| kind_node(graph, id)) {
        for row in detail_rows(graph, node) {
            let tag = format!("{GRAPH_TAG}#detail_{}", row.key);
            group = group.with_child(tag.clone());
            let mut entry = AccessNode::new(tag, AriaRole::Generic)
                .with_name(format!("{}: {}", row.label, row.value));
            if panel_edit == Some(row.target) {
                entry = entry.with_child(EDIT_TF_TAG);
                editor = Some(
                    tf_paint::text_field_a11y_node(
                        EDIT_TF_TAG,
                        use_text_edit_state(EDIT_TF_TAG).text(),
                        field_state,
                        focused == Some(EDIT_TF_TAG),
                    )
                    .with_name(row.label),
                );
            }
            rows_out.push(entry);
        }
    }
    let mut out = vec![group];
    out.append(&mut rows_out);
    out.extend(editor);
    out
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
    fn access_node(state: &RootState, focused: Option<&str>) -> Vec<AccessNode> {
        let graph = use_document().get();
        let selection = use_selection().get();
        let selected = selection.nodes();
        let active = use_active_edit().get();
        // R918 — only a card-surface edit hosts the field on a node card; a
        // panel-surface edit hosts it in the Details panel subtree instead.
        let card_edit = active
            .filter(|a| a.surface == EditSurface::Card)
            .map(|a| a.target);
        // R849/R850 / R918 — the editor lowers to a root with three regions: the
        // add-node palette (a `toolbar` of `button`s), the graph canvas (the R840
        // unordered `group` of node `generic`s), and the Details panel `group`.
        let root = AccessNode::new(ROOT_TAG, AriaRole::Group)
            .with_name("Node editor")
            .with_child(PALETTE_TAG)
            .with_child(GRAPH_TAG)
            .with_child(DETAIL_TAG);
        // R850 — the palette `toolbar` + `button`s come from the
        // `toolbar_button_nodes` SSOT (gaining aria-posinset/setsize), not a
        // hand-rolled equivalent. `focused_control: None` because the palette is
        // not yet a keyboard tab stop (only the canvas is marked
        // `.with_focusable(true)`) — a
        // mouse/RPC-driven toolbar, the `hello-textarea` NoFocus-toolbar
        // precedent. Keyboard roving over the palette is a documented carry.
        let palette_tags: Vec<String> = (0..PALETTE.len())
            .map(|i| format!("{GRAPH_TAG}#palette_{i}"))
            .collect();
        let palette_names: Vec<String> = PALETTE.iter().map(|&(t, _)| format!("Add {t}")).collect();
        let controls: Vec<ToolbarControl> = palette_tags
            .iter()
            .zip(&palette_names)
            .map(|(tag, name)| ToolbarControl {
                tag: tag.as_str(),
                name: Some(name.as_str()),
                checked: None,
                disabled: false, // every palette entry is always operable (R989)
            })
            .collect();
        let mut out = vec![root];
        out.extend(toolbar_button_nodes(
            PALETTE_TAG,
            "Add node",
            &controls,
            None,
        ));
        // R1220 — the open pin-drop create menu, computed once (its source pin
        // must still be valid) for both the group-child link below and the
        // trailing `menu` subtree, so the AT tree advertises it exactly when it
        // paints. Carries `(highlight, candidate kind indices)`.
        let pin_menu = use_pin_create().get().and_then(|pc| {
            let from_ty = output_type(&graph, pc.from_node, pc.from_port)?;
            Some((pc.highlight, pin_create_candidates(from_ty, &pc.filter)))
        });
        // R879 — the canvas owns a multi-selection set; announce it
        // (`aria-multiselectable`) so per-node `aria-selected` flags read
        // as set membership, not a single highlight.
        let mut group = AccessNode::new(GRAPH_TAG, AriaRole::Group)
            .with_name("Node graph")
            .with_multiselectable()
            .with_state(AccessState {
                focused: focused == Some(GRAPH_TAG),
                ..AccessState::default()
            });
        for (node, _) in kind_nodes(&graph) {
            group = group.with_child(format!("{GRAPH_TAG}#node_{}", node.id));
        }
        // R1227 — the comment frames are also children of the canvas group (a
        // labelled `group` per frame, reachable in the AT tree, not an orphan).
        for f in frame_nodes(&graph) {
            group = group.with_child(format!("{GRAPH_TAG}#frame_{}", f.id));
        }
        // R1220 — link the open create menu into the canvas group so its
        // `menu` subtree is reachable (not an orphan in the AT tree).
        if pin_menu.is_some() {
            group = group.with_child(format!("{GRAPH_TAG}#{PIN_MENU_SUB}"));
        }
        out.push(group);
        out.extend(frame_access_nodes(&graph));
        for (node, op) in kind_nodes(&graph) {
            let mut entry =
                AccessNode::new(format!("{GRAPH_TAG}#node_{}", node.id), AriaRole::Generic)
                    .with_name(format!(
                        "{} ({} in, {} out)",
                        node.title(),
                        op.inputs().len(),
                        op.outputs().len()
                    ))
                    .with_selected(selected.contains(&node.id));
            // R878 / R901 — while this node hosts a *card*-surface inline edit,
            // the shared field is its child textbox (the lifted
            // `text_field_a11y_node` SSOT), named for the edit kind ("Rename
            // node" for a title, "Port default" for a pin default). Gated on
            // the SAME `card_edit` predicate the card paint uses, so the AT tree
            // never advertises an unpainted editor. A panel-surface edit hosts
            // the field in the Details subtree instead (R918).
            if card_edit.is_some_and(|t| t.node() == node.id) {
                let name = match card_edit {
                    Some(EditTarget::PortDefault { .. }) => "Port default",
                    Some(EditTarget::SourceConst(_)) => "Source value",
                    _ => "Rename node",
                };
                entry = entry.with_child(EDIT_TF_TAG);
                out.push(
                    tf_paint::text_field_a11y_node(
                        EDIT_TF_TAG,
                        use_text_edit_state(EDIT_TF_TAG).text(),
                        state.0,
                        focused == Some(EDIT_TF_TAG),
                    )
                    .with_name(name),
                );
            }
            out.push(entry);
        }
        // R918 — the Details panel subtree (rows + the in-panel editor when a
        // panel edit is in flight).
        out.extend(details_access_nodes(
            &graph, &selection, active, state.0, focused,
        ));
        // R1220 — the open create menu's `menu` + `menuitem` subtree.
        if let Some((highlight, candidates)) = pin_menu {
            out.extend(pin_create_access_nodes(highlight, &candidates));
        }
        out
    }
}

/// R1220 — the pin-drop create menu as an ARIA `menu` of `menuitem`s (the
/// [`menu_item_nodes`] SSOT), each item byte-matching its painted
/// `node_graph#create_<idx>` card so the AT tree advertises the menu exactly when
/// and where it paints; the highlighted row (`highlight`, into `candidates`) is
/// the roving `focused` item. Split out of the `access_node` builder to keep it
/// under the line budget.
fn pin_create_access_nodes(highlight: usize, candidates: &[usize]) -> Vec<AccessNode> {
    let tags: Vec<String> = candidates
        .iter()
        .map(|&k| format!("{GRAPH_TAG}#create_{k}"))
        .collect();
    let cells: Vec<MenuItemCell> = candidates
        .iter()
        .enumerate()
        .map(|(row, &k)| MenuItemCell {
            tag: tags[row].as_str(),
            label: Some(PALETTE[k].0),
            focused: row == highlight,
            ..MenuItemCell::default()
        })
        .collect();
    menu_item_nodes(
        &format!("{GRAPH_TAG}#{PIN_MENU_SUB}"),
        "Create node",
        &cells,
    )
}

impl WidgetView for NodeEditorView {
    type Renderer = HelloNodeEditorRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        // R916 — the window fits the palette + canvas + the Details panel
        // ([`TOTAL_W`], the same value the root container declares), so the flex
        // row never shrinks: the canvas keeps its `WIN_W` width at the
        // `PALETTE_W` offset, so the node / wire geometry is unchanged.
        pinion_shell::SizeStrategy::Fixed {
            width: TOTAL_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<NodeEditorView>();
}

#[cfg(test)]
mod tests;
