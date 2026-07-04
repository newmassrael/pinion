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
//! `edge_ids` / `node.<id>.{title,x,y,inputs,outputs,input_types,output_types,input_default.<port>}` /
//! `edge.<id>` / `selected` / `selected_ids` / `selected_edge`;
//! `intervene node.<id>.x` / `node.<id>.y` / `node.<id>.title` /
//! `node.<id>.input_default.<port>` / `frame.<id>.{title,x,y,w,h}` (R1234:
//! `x`/`y` move the frame + its contents, `w`/`h` resize it) /
//! `selected` / `selected_ids` /
//! `selected_edge`; `invoke add_edge` / `remove_edge` /
//! `reconnect_edge` / `add_reroute` (R1235: splice a typed passthrough into an
//! edge) / `delete_node` / `delete_selected` / `dissolve_node` /
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
//!   for now (`Float`, `Vector`); `PortType` is `#[non_exhaustive]`, so a
//!   blueprint consumer's `Exec` / `Bool` extend the lattice without a
//!   re-spell ([[abstraction-needs-second-consumer]] — the taxonomy is the
//!   consumer's, the mechanism is shared).
//! - **Port default values** (R899): each input port carries a typed literal
//!   default (the "pin default value"), typed by its [`PortType`] — a `Float`
//!   port a scalar, a `Vector` port a colour — reusing the data-grid
//!   [`CellValue`] value substrate. An **unconnected** port paints its default
//!   beside the pin (a wired port hides it; the value is retained); it is
//!   AI-read/write (`query`/`intervene node.<id>.input_default.<port>`, typed —
//!   a colour takes a `#RRGGBB[AA]` hex, the write journals an undoable
//!   [`SetPortDefaultCmd`]). The painted default is **inline-editable on the
//!   canvas** (R901): a **double-click** on the default opens the shared inline
//!   field (the same affordance the title rename uses, R878), while the input
//!   port's *single*-click stays edge-connect (R742); the AI path is the
//!   `intervene node.<id>.input_default.<port>` write above.
//!   Genuinely-separate axes still deferred: dataflow **evaluation** (a Phase-C
//!   *runtime* concern — this is the *authoring* substrate, not the compute
//!   engine) and the typed ports' AT enrichment (the a11y name keeps the arity
//!   count: `pinion-a11y` has no diagram / `graphics-document` role yet — an
//!   upstream substrate gap, the same one the module-level a11y note records).
//! - **Undo / redo** (R851 + R853): every edit is reversible on the shared
//!   [`UndoStack`] — the **structural** edits (add node, delete node + its
//!   incident edges, connect, disconnect) as [`GraphEdit`] deltas, and node
//!   **moves** (drag / nudge / `intervene .x`) as [`MoveNodesCmd`]s. A drag is
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
//!   opened document is a fresh baseline — the `QUndoStack` model).
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
//!   dragged nodes stay pinned under the cursor — the DCC / Unreal
//!   drag-past-the-edge convention. Wire-connect auto-pan (the `begin_drag`
//!   DnD path) stays a documented follow-up.
//! - **Inline edit** (R878 title / R901 port default): double-click a node
//!   card (or `F2` on the selection, or `invoke begin_rename`) edits the
//!   title; double-click a pin's default label (or `invoke
//!   begin_edit_default`) edits that input port's literal default. Both open
//!   ONE shared inline [`TextFieldExternal`] (the R790 todomvc `EDIT_TF`
//!   modal-member shape) keyed by an [`EditTarget`]. `Enter` commits,
//!   `Escape` cancels, a click-away commits through the R793 blur intent; the
//!   keymap is the lifted [`pinion_core::edit_field_keymap`] SSOT with the
//!   target's typed [`CellKind`] gate (title = text, a `Float` port = a
//!   number, a `Vector`/`Color` port = a `#RRGGBB[AA]` hex). A commit journals
//!   an undoable [`RenameCmd`] / [`SetPortDefaultCmd`] through the
//!   `apply_rename` / `apply_set_default` SSOT — the same path the AI-first
//!   `intervene node.<id>.title` / `node.<id>.input_default.<port>` write-twins
//!   drive (`query editing` is the in-flight read; `query renaming` survives as
//!   its title-only projection).
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
use pinion_core::composite_tag::{split_send_payload, split_subindex};
use pinion_core::event::LINE_HEIGHT_PX;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, DragPayload, DropPoint, External,
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    RepaintOwner, ThreadOwnership, int_of,
};
use pinion_core::reactive::{Owner, Signal, batch};
use pinion_core::scene::{
    ContainerNode, PathCommand, PathNode, PathPoint, Rect, ScrollAxis, ScrollNode, TextNode,
};
use pinion_core::storage::Storage;
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, PathStyle, Size,
    Stroke, StrokeCap, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::undo::{
    UndoCommand, UndoStack, UndoStackExternal, undo_redo_verb, use_undo_stack,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Color, Command, DragLatch, Frame, Modifiers, Scene, SelectionChord, WidgetCore};
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
/// `add_node` RPC verb) can create, as `(title, input_types, output_types)`.
/// A tiny material-graph vocabulary (sources / ops / sink), the same typed
/// port shapes [`default_nodes`] seeds. R898 — the entries are now
/// [`PortType`]-typed; the first five keep their pre-R898 indices (0..=4) so
/// the index-addressed `add_node(kind)` callers are unmoved, and the typed
/// sources/ops (`Scalar`, `Lerp`) that exercise the type lattice are
/// *appended* (5, 6).
const PALETTE: &[(&str, &[PortType], &[PortType])] = &[
    ("Texture", &[], &[PortType::Vector]),
    ("Color", &[], &[PortType::Vector]),
    (
        "Multiply",
        &[PortType::Vector, PortType::Vector],
        &[PortType::Vector],
    ),
    (
        "Add",
        &[PortType::Vector, PortType::Vector],
        &[PortType::Vector],
    ),
    ("Output", &[PortType::Vector], &[]),
    // R898 — a scalar source: its `Float` output broadcasts into a `Vector`
    // input (accepted) but a `Vector` never narrows into it (rejected).
    ("Scalar", &[], &[PortType::Float]),
    // R898 — a 3-input op whose last input is a `Float` factor, so a
    // `Color`/`Texture` (`Vector`) wired to it is type-rejected.
    (
        "Lerp",
        &[PortType::Vector, PortType::Vector, PortType::Float],
        &[PortType::Vector],
    ),
];

/// R849 — sidebar width for the node palette. The canvas keeps its
/// `WIN_W × WIN_H` coordinate system; the palette is an extra left strip, so
/// the capture-drag / hit-test canvas-extent math is unchanged (the
/// `capture_normalize` reference is the offset `GRAPH_TAG` rect).
const PALETTE_W: u32 = 132;
/// R849 — the palette container tag (a11y `toolbar` root; routes no pointer
/// events itself — only its `node_graph#palette_<idx>` item cards do).
const PALETTE_TAG: &str = "node_palette";
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

/// R878 / R901 — the shared inline edit field (a [`TextFieldExternal`] extra,
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
// R899 -> 3: added per-port `input_defaults` (typed `CellValue`s). Each bump
// mismatch-rejects a stale blob so it starts fresh rather than misreading.
const PERSISTED_SCHEMA_VERSION: u32 = 4;

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
/// canvas every desktop node editor uses (Unreal's blueprint graph is
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
/// toward that edge (the DCC / Unreal convention — a node dragged to the rim
/// keeps going without releasing). 0.12 of the 640×420 canvas ≈ a 77 px / 50 px
/// rim.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
struct NodeId(u32);

/// Stable edge handle (R841), same discipline as [`NodeId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct EdgeId(u32);

/// R1227 — stable comment-frame handle, same stable-id discipline as
/// [`NodeId`] / [`EdgeId`] (minted once, never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct FrameId(u32);

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

impl FrameId {
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

impl core::fmt::Display for FrameId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// R898 — a port's data type. The lattice that makes the graph a *typed*
/// node editor: an edge connects an output to an input only when the
/// output's type is assignable to the input's (see [`PortType::is_assignable_to`]),
/// so the canvas rejects an ill-typed wire the way Unreal's blueprint /
/// material graphs do. Two material-graph types for now (`Float` scalar,
/// `Vector` colour/vec3); `#[non_exhaustive]` so a blueprint consumer's
/// `Exec` / `Bool` extend it without re-spelling the match arms here
/// ([[abstraction-needs-second-consumer]] — the type *taxonomy* is the
/// consumer's, the *mechanism* is shared).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
enum PortType {
    /// A scalar.
    Float,
    /// A vec3 / colour (a material graph's primary data type).
    Vector,
}

/// R898 — a `Float` port's signature colour. Node editors colour-code pins by
/// type (Unreal/Blender) so a connection's validity is legible at a glance;
/// the colour is a fixed type identity, *not* themed (a `Float` reads the same
/// green in light and dark).
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
    const fn is_assignable_to(self, into: PortType) -> bool {
        // `Float` assigns to `Float` or `Vector` (exact + scalar broadcast);
        // `Vector` only to `Vector` (no narrowing).
        matches!(
            (self, into),
            (PortType::Float, PortType::Float | PortType::Vector)
                | (PortType::Vector, PortType::Vector)
        )
    }
}

/// R1220 — the first input port index of [`PALETTE`] kind `kind` an output of
/// type `from` may feed (the auto-wire target when a pin-drop creates that
/// node). `None` when the kind has no input assignable from `from` — the exact
/// gate [`pin_create_candidates`] filters on, so a returned candidate always
/// resolves a wire target here.
fn first_compatible_input(kind: usize, from: PortType) -> Option<usize> {
    let &(_, input_ports, _) = PALETTE.get(kind)?;
    input_ports.iter().position(|&t| from.is_assignable_to(t))
}

/// R1220 — the [`PALETTE`] kinds a pin-drop from an output of type `from` may
/// create, in palette order: a kind qualifies iff it has at least one input
/// port assignable from `from` (so the new node can be auto-wired —
/// [`first_compatible_input`] is guaranteed `Some`), and, when `filter` is
/// non-empty, its title contains `filter` (case-insensitive) — the type-to-narrow
/// search the Unreal / Blender pin-drop menu offers. A pure fn over `(from,
/// filter)` so both the coordinator (menu candidates + commit gate) and the tests
/// read one SSOT.
fn pin_create_candidates(from: PortType, filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    (0..PALETTE.len())
        .filter(|&k| first_compatible_input(k, from).is_some())
        .filter(|&k| {
            needle.is_empty() || PALETTE[k].0.to_ascii_lowercase().contains(needle.as_str())
        })
        .collect()
}

/// One node: a titled card with typed input ports (left edge) and typed
/// output ports (right edge), placed at canvas `(x, y)`. Carries a stable
/// [`NodeId`]. R898 — the ports are [`PortType`]-typed sockets; the arity
/// (`inputs()` / `outputs()`) is now the *length* of those lists, so the
/// pre-R898 count-only contract is the degenerate read of the richer model.
// R899 — no `Eq`: `input_defaults` carries [`CellValue`]s (an `f64` `Float`
// arm), so the model is `PartialEq` only. Never a set / map key, so `Eq` is
// not required (the same reason `SerializedGraph` / `GraphDelta` below drop it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct GraphNode {
    id: NodeId,
    title: String,
    x: i32,
    y: i32,
    input_ports: Vec<PortType>,
    output_ports: Vec<PortType>,
    /// R899 — the literal default per input port (parallel to `input_ports`),
    /// used when the port is unconnected (the "pin default value"). Typed by
    /// the port's [`PortType`]; retained while the port is wired (the Unreal
    /// model — wiring hides the editor, it does not discard the value).
    input_defaults: Vec<CellValue>,
}

impl GraphNode {
    fn new(
        id: u32,
        title: &str,
        x: i32,
        y: i32,
        inputs: &[PortType],
        outputs: &[PortType],
    ) -> Self {
        Self {
            id: NodeId(id),
            title: title.to_owned(),
            x,
            y,
            input_ports: inputs.to_vec(),
            output_ports: outputs.to_vec(),
            input_defaults: inputs.iter().map(|t| t.default_value()).collect(),
        }
    }

    /// R899 — the literal default of input port `port` (`None` out of range).
    fn input_default(&self, port: usize) -> Option<&CellValue> {
        self.input_defaults.get(port)
    }

    /// Input-port arity — the pre-R898 `inputs` count, now derived from the
    /// typed list so the RPC `node.<id>.inputs` contract stays byte-stable.
    fn inputs(&self) -> usize {
        self.input_ports.len()
    }

    /// Output-port arity (the derived count twin of [`GraphNode::inputs`]).
    fn outputs(&self) -> usize {
        self.output_ports.len()
    }

    /// The type of input port `port`, if it exists (the `add_edge` type gate).
    fn input_type(&self, port: usize) -> Option<PortType> {
        self.input_ports.get(port).copied()
    }

    /// The type of output port `port`, if it exists.
    fn output_type(&self, port: usize) -> Option<PortType> {
        self.output_ports.get(port).copied()
    }

    /// Port rows = the taller of the two columns (at least one, so a
    /// source / sink node still has a body).
    fn rows(&self) -> i32 {
        irow(self.inputs().max(self.outputs()).max(1))
    }

    fn height(&self) -> i32 {
        HEADER_H + self.rows() * PORT_PITCH + BODY_PAD
    }

    /// R880.1 — the card's right extent in graph units. The bounds
    /// expressions (`x + NODE_W` / `y + height()`) were re-derived at
    /// three sites (frame_all, the marquee rect-hit, tests); the accessor
    /// pair is their one home.
    const fn right(&self) -> i32 {
        self.x + NODE_W
    }

    /// R880.1 — the card's bottom extent in graph units.
    fn bottom(&self) -> i32 {
        self.y + self.height()
    }
}

/// R948 — the union bounding box `(left, top, right, bottom)` in graph units
/// of a node set (each node spans `x..right()` × `y..bottom()`), or `None`
/// for an empty set. The one home for the min/max fold that `frame_all` (all
/// nodes) and `align_selected` (the selection) both need — a divergent fold
/// would let the two read a node's extent differently ([[ssot-lift-grep-repo-wide-cross-enum]]).
fn node_bounds<'a>(nodes: impl Iterator<Item = &'a GraphNode>) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for n in nodes {
        bounds = Some(match bounds {
            None => (n.x, n.y, n.right(), n.bottom()),
            Some((l, t, r, b)) => (l.min(n.x), t.min(n.y), r.max(n.right()), b.max(n.bottom())),
        });
    }
    bounds
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

/// R1227 — a **comment frame**: a titled, translucent rectangle drawn BEHIND the
/// nodes to group + label a region of the graph (the Blueprint / material-editor
/// "comment box"). It owns no graph semantics — it is a pure annotation over
/// graph units (`x`,`y`,`w`,`h`), created around a node selection and moved with
/// the nodes it visually contains. Stable [`FrameId`] (R841 discipline), so a
/// frame reference survives the deletion of others; persisted in the
/// [`SerializedGraph`] like nodes + edges.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CommentFrame {
    id: FrameId,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    title: String,
}

impl CommentFrame {
    /// The frame's rect right / bottom edges (graph units).
    const fn right(&self) -> i32 {
        self.x + self.w
    }
    const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Whether node `n`'s CENTRE lies inside this frame's rect — the
    /// "contained" test the move-with-contents + the `contains` query share, so
    /// the painted membership and the moved set can never disagree. Centre (not
    /// full-overlap) is the Blueprint rule: a node belongs to the frame it sits
    /// in, even if its card slightly overhangs the border.
    fn contains_node(&self, n: &GraphNode) -> bool {
        let cx = n.x + NODE_W / 2;
        let cy = n.y + n.height() / 2;
        cx >= self.x && cx <= self.right() && cy >= self.y && cy <= self.bottom()
    }
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

/// The id new edges mint from: one past the highest [`default_edges`] id.
/// Derived (not a hand-maintained const) so it can never drift out of sync
/// with the defaults — adding a seed edge cannot silently collide a minted id.
fn first_dynamic_edge_id() -> u32 {
    default_edges()
        .iter()
        .map(|e| e.id.raw())
        .max()
        .map_or(0, |m| m + 1)
}

/// R849 — the id new nodes mint from: one past the highest [`default_nodes`]
/// id. Derived (mirroring [`first_dynamic_edge_id`]) so adding a seed node
/// cannot silently collide a minted id.
fn first_dynamic_node_id() -> u32 {
    default_nodes()
        .iter()
        .map(|n| n.id.raw())
        .max()
        .map_or(0, |m| m + 1)
}

/// First-paint graph — a tiny material graph (`Texture` × `Color` →
/// `Multiply` → `Output`). Node ids 0..=3, edge ids 0..=2.
fn default_nodes() -> Vec<GraphNode> {
    use PortType::Vector;
    vec![
        GraphNode::new(0, "Texture", 40, 70, &[], &[Vector]),
        GraphNode::new(1, "Color", 40, 210, &[], &[Vector]),
        GraphNode::new(2, "Multiply", 250, 110, &[Vector, Vector], &[Vector]),
        GraphNode::new(3, "Output", 470, 150, &[Vector], &[]),
    ]
}

fn default_edges() -> Vec<Edge> {
    vec![
        Edge {
            id: EdgeId(0),
            from_node: NodeId(0),
            from_port: 0,
            to_node: NodeId(2),
            to_port: 0,
        },
        Edge {
            id: EdgeId(1),
            from_node: NodeId(1),
            from_port: 0,
            to_node: NodeId(2),
            to_port: 1,
        },
        Edge {
            id: EdgeId(2),
            from_node: NodeId(2),
            from_port: 0,
            to_node: NodeId(3),
            to_port: 0,
        },
    ]
}

/// R852 — the persistable graph snapshot. Carries the nodes, the edges, **and**
/// the monotonic id counters, so a reload resumes minting where the saved
/// session left off (a deleted-then-saved id is never handed out again). The
/// selection is transient UI state and is *not* persisted. `schema_version`
/// gates the load: a mismatch starts fresh rather than misreading an old layout.
// R899 — `PartialEq` only (it carries `GraphNode`s with their `CellValue`
// `input_defaults`); never a set / map key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SerializedGraph {
    schema_version: u32,
    nodes: Vec<GraphNode>,
    edges: Vec<Edge>,
    next_node_id: u32,
    next_edge_id: u32,
    /// R1227 — the comment frames + their id counter. `#[serde(default)]` lets
    /// the *current* (schema-4) format tolerate a frames-less blob during the
    /// deserialize step. R1232 — it does NOT forward-load an old save: `load_json`
    /// rejects any `schema_version != PERSISTED_SCHEMA_VERSION` BEFORE the field
    /// defaults matter, so a pre-R1227 (schema-3) blob is version-rejected (the
    /// strict "a foreign document version does not open" contract the 1→2 rename /
    /// 2→3 semantic bumps established), not migrated to empty frames.
    #[serde(default)]
    frames: Vec<CommentFrame>,
    #[serde(default)]
    next_frame_id: u32,
}

// ─── geometry (window coordinates; canvas == window) ───────────────

/// The vertical offset of port row `row` within a node card (top of the
/// port box).
fn port_row_top(row: usize) -> i32 {
    HEADER_H + irow(row) * PORT_PITCH + PORT_PITCH / 2 - PORT_SIZE / 2
}

/// Centre of input port `i` of `node`, in window coordinates.
fn input_port_center(node: &GraphNode, i: usize) -> (i32, i32) {
    (
        node.x + PORT_SIZE / 2,
        node.y + port_row_top(i) + PORT_SIZE / 2,
    )
}

/// Centre of output port `j` of `node`, in window coordinates.
fn output_port_center(node: &GraphNode, j: usize) -> (i32, i32) {
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

/// R877 — node positions clamp to the WORLD extent, not the window: the
/// canvas pans, so the old window-extent clamp would have pinned every
/// node inside the boot view. The unsigned scene substrate makes `0` the
/// world's hard left/top edge; the right/bottom clamp keeps the whole
/// card on the world surface.
fn clamp_node_x(x: i32) -> i32 {
    x.clamp(0, WORLD - NODE_W)
}

fn clamp_node_y(y: i32) -> i32 {
    y.clamp(0, WORLD - HEADER_H - 8)
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

/// R1227 — the shared comment-frame list (empty at boot; frames are authored,
/// not seeded), the same `Rc<Signal>` the view fn reads + the coordinator mutates.
fn use_frames() -> Rc<Signal<Vec<CommentFrame>>> {
    let owner = Owner::current().expect("use_frames requires an active Owner scope");
    owner.cache("node_graph.frames", || Signal::new(Vec::new()))
}

/// R1227 — monotonic [`FrameId`] source (mirrors [`use_next_node_id`]); frames
/// mint from 0 since the boot graph seeds none.
fn use_next_frame_id() -> Rc<Cell<u32>> {
    let owner = Owner::current().expect("use_next_frame_id requires an active Owner scope");
    owner.cache("node_graph.next_frame_id", || Cell::new(0))
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

/// R1220 — the in-flight pin-drop create menu (`None` when closed): the
/// signature Unreal / Blender authoring gesture — drag a wire off an output pin,
/// release on empty canvas, and a type-filtered menu of the nodes that output can
/// feed opens; pick one and it is created at the drop point AND auto-wired in one
/// undo step. Written by the coordinator (the live [`NodeGraphExternal::drag_release`]
/// empty-canvas branch and the `open_pin_create` RPC verb), read by the view fn
/// (which paints the floating menu — reading it subscribes the paint Effect, so
/// open / filter / highlight changes repaint) and the keyboard path. Transient UI
/// state, like [`use_preview`] / [`use_marquee_rect`].
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
}

impl EditTarget {
    /// The node whose property this target edits. The migration-commit, the
    /// paint switch, and the a11y child all key off this id.
    fn node(self) -> NodeId {
        match self {
            EditTarget::Title(id)
            | EditTarget::PortDefault { node: id, .. }
            | EditTarget::PosX(id)
            | EditTarget::PosY(id) => id,
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
    /// A Details panel row (R918) — the Unreal "click a property to edit it"
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

/// Monotonic [`EdgeId`] allocator — persists across view-fn re-runs so a
/// minted id is never reused (the stable-identity guarantee for new edges).
#[must_use]
fn use_next_edge_id() -> Rc<Cell<u32>> {
    let owner = Owner::current().expect("use_next_edge_id requires an active Owner scope");
    owner.cache("node_graph.next_edge_id", || {
        Cell::new(first_dynamic_edge_id())
    })
}

/// R849 — the monotonic [`NodeId`] source for newly created nodes (mirrors
/// [`use_next_edge_id`]). One shared `Cell` per Owner scope so a minted id is
/// never reused, even across deletes.
fn use_next_node_id() -> Rc<Cell<u32>> {
    let owner = Owner::current().expect("use_next_node_id requires an active Owner scope");
    owner.cache("node_graph.next_node_id", || {
        Cell::new(first_dynamic_node_id())
    })
}

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
/// clock (idempotent; the [`use_caret_blink`] *register-once mechanism* is the
/// precedent — the rest-semantics differ, see [`AutoPan::is_at_rest`]). Called
/// from the view setup so it ticks on every paint cycle, self-gating via
/// [`AutoPan::active`].
fn use_autopan() {
    let owner = Owner::current().expect("use_autopan requires an active Owner scope");
    let nodes = use_nodes();
    let scroll = use_canvas_scroll();
    let zoom = use_zoom();
    let node_drag = use_node_drag();
    let _: Rc<AutoPan> = owner.register_animation_once("node_graph.autopan", move || AutoPan {
        nodes,
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
    nodes: Rc<Signal<Vec<GraphNode>>>,
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
            follow_members(&self.nodes, &start.members, gx, gy);
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
/// composite sub-tag (the [`view_input_default`] hit target). Double-clicking
/// it opens the inline default editor (the [`parse_oport_sub`] peer over the
/// `idefault_` prefix).
fn parse_idefault_sub(sub: &str) -> Option<(NodeId, usize)> {
    split_node_port(sub.strip_prefix("idefault_")?)
}

/// A full drop tag `node_graph#iport_<id>_<i>` → (node id, input port). Uses
/// the canonical `#` splitter (`split_subindex`) rather than an inline split.
fn parse_input_port_tag(tag: &str) -> Option<(NodeId, usize)> {
    split_node_port(split_subindex(tag).1?.strip_prefix("iport_")?)
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

/// R898 — join a port's types into the CSV the `node.<id>.input_types` /
/// `output_types` queries return ("Vector,Float"; "" for a source / sink).
fn port_types_csv(ports: &[PortType]) -> String {
    ports.iter().map(|p| p.name()).collect::<Vec<_>>().join(",")
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
    /// anchor); grabbing an unselected node drags just it (the Unreal /
    /// QGraphicsView group-move convention). Id-sorted by construction
    /// (selection-set order) so `end_gesture`'s journal entry matches
    /// [`MoveNodesCmd::merge`]'s same-member ordering.
    members: Vec<(NodeId, f64, f64, i32, i32)>,
    /// R879 audit fix / R880 — the dead zone: the framework [`DragLatch`]
    /// (the SAME contract predicate the router and the marquee advance).
    /// Until it latches, members do NOT move (the Qt `startDragDistance`
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
    /// R850 — a press on an add-node palette card. Like [`Inert`](Self::Inert)
    /// it is a non-drag press that must suppress the background edge-probe, but
    /// it is recorded as its own variant rather than a bare inert press because
    /// its release has a specific action (it creates a node), so a future branch
    /// can never misread a palette press as an inert one.
    Palette,
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
    /// R1227 — comment frames added / removed by this edit (the `add_frame` /
    /// `remove_frame` structural edits; stored verbatim so undo re-inserts them
    /// with their original [`FrameId`]).
    added_frames: Vec<CommentFrame>,
    removed_frames: Vec<CommentFrame>,
}

struct GraphEdit {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    edges: Rc<Signal<Vec<Edge>>>,
    /// R1227 — the frame list, so `add_frame` / `remove_frame` round-trip on the
    /// same shared undo path as node / edge edits.
    frames: Rc<Signal<Vec<CommentFrame>>>,
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
    /// Apply the delta in the `redo` direction (`reverse == false`: drop
    /// `removed_*`, add `added_*`) or its inverse (`reverse == true`), then set
    /// the selection. Taking the whole [`GraphDelta`] + a direction bit keeps the
    /// signature within the argument budget as the entity kinds grow (R1227 added
    /// frames — a per-kind param list would have overflowed).
    fn apply(&self, delta: &GraphDelta, reverse: bool, sel: Selection) {
        let (rm_nodes, add_nodes) = if reverse {
            (&delta.added_nodes, &delta.removed_nodes)
        } else {
            (&delta.removed_nodes, &delta.added_nodes)
        };
        let (rm_edges, add_edges) = if reverse {
            (&delta.added_edges, &delta.removed_edges)
        } else {
            (&delta.removed_edges, &delta.added_edges)
        };
        let (rm_frames, add_frames) = if reverse {
            (&delta.added_frames, &delta.removed_frames)
        } else {
            (&delta.removed_frames, &delta.added_frames)
        };
        if !rm_nodes.is_empty() || !add_nodes.is_empty() {
            self.nodes.set_with(|prev| {
                let mut next: Vec<GraphNode> = prev
                    .iter()
                    .filter(|n| !rm_nodes.iter().any(|r| r.id == n.id))
                    .cloned()
                    .collect();
                next.extend(add_nodes.iter().cloned());
                next
            });
        }
        if !rm_edges.is_empty() || !add_edges.is_empty() {
            self.edges.set_with(|prev| {
                let mut next: Vec<Edge> = prev
                    .iter()
                    .copied()
                    .filter(|e| !rm_edges.iter().any(|r| r.id == e.id))
                    .collect();
                next.extend(add_edges.iter().copied());
                next
            });
        }
        if !rm_frames.is_empty() || !add_frames.is_empty() {
            self.frames.set_with(|prev| {
                let mut next: Vec<CommentFrame> = prev
                    .iter()
                    .filter(|f| !rm_frames.iter().any(|r| r.id == f.id))
                    .cloned()
                    .collect();
                next.extend(add_frames.iter().cloned());
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
        self.apply(&self.delta, false, self.sel_after.clone());
    }

    fn undo(&self) {
        self.apply(&self.delta, true, self.sel_before.clone());
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
/// R879 — one journaled member move: `(id, before, after)` window positions.
type NodeMove = (NodeId, (i32, i32), (i32, i32));

/// R948 — which edge / centre line of the selection's bounding box an
/// `align_*` snaps every selected node to. Horizontal specs move only `x`,
/// vertical specs only `y` — the canonical Qt / Blender / Figma align set.
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
/// *centres* evenly along x (`Horizontal`) or y (`Vertical`), holding the two
/// extreme members fixed (the PowerPoint / Figma "distribute centres" rule).
#[derive(Clone, Copy)]
enum DistributeAxis {
    Horizontal,
    Vertical,
}

/// R948 — twice a node's centre on `axis` (`2x + NODE_W` h / `2y + height()`
/// v). Doubling keeps the key an integer so the distribute sort + position math
/// use a total `Ord` key with no float compare; the `/2` only re-enters at the
/// final px target.
fn centre_key(n: &GraphNode, axis: DistributeAxis) -> i32 {
    match axis {
        DistributeAxis::Horizontal => 2 * n.x + NODE_W,
        DistributeAxis::Vertical => 2 * n.y + n.height(),
    }
}

/// R879 — generalises the R853 single-node `MoveNodeCmd`: one undo step
/// holding one `(id, before, after)` per moved member, so a multi-select
/// drag / nudge is exactly ONE journal entry (the Unreal / Qt group-move
/// undo shape) while a single-node move is the one-element case.
struct MoveNodesCmd {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    /// `(id, before, after)` per moved node — id-sorted by construction
    /// (built from the selection's `BTreeSet` order), which `merge`'s
    /// same-member check relies on.
    moves: Vec<NodeMove>,
    coalescable: bool,
}

impl MoveNodesCmd {
    /// Set every member to its `before` (`to_after = false`) or `after`
    /// position in one signal write (no clamp — both ends were captured
    /// from already-clamped positions). An absent member is skipped (a
    /// LIFO undo can never reach a move while the node is deleted, but
    /// the signal write stays total either way).
    fn set_all(&self, to_after: bool) {
        self.nodes.set_with(|prev| {
            let mut next = prev.clone();
            for (id, before, after) in &self.moves {
                let pos = if to_after { after } else { before };
                if let Some(n) = next.iter_mut().find(|n| n.id == *id) {
                    n.x = pos.0;
                    n.y = pos.1;
                }
            }
            next
        });
    }
}

impl UndoCommand for MoveNodesCmd {
    fn label(&self) -> Cow<'static, str> {
        if self.moves.len() == 1 {
            Cow::Borrowed("Move node")
        } else {
            Cow::Owned(format!("Move {} nodes", self.moves.len()))
        }
    }

    fn redo(&self) {
        self.set_all(true);
    }

    fn undo(&self) {
        self.set_all(false);
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }

    /// Fold a contiguous same-member move into this one: extend each
    /// `after` to the successor's. Only when both ends are `coalescable`,
    /// target the identical member list, and are contiguous per member —
    /// so a drag (non-coalescable) breaks the run on either side, and a
    /// move of a different selection starts a fresh undo step.
    fn merge(&mut self, next: &dyn UndoCommand) -> bool {
        let Some(next) = next.as_any().and_then(|a| a.downcast_ref::<MoveNodesCmd>()) else {
            return false;
        };
        if !(self.coalescable && next.coalescable && self.moves.len() == next.moves.len()) {
            return false;
        }
        // R856 — require same member + contiguity (`self.after ==
        // next.before`) per element, like the substrate's canonical
        // `AddCmd::merge`: every current caller captures `before` from live
        // state, so the run is contiguous by construction, but the guard
        // keeps a future stale-`before` caller from folding a
        // non-contiguous run and losing the intermediate position on undo.
        let foldable = self
            .moves
            .iter()
            .zip(&next.moves)
            .all(|((id_a, _, after), (id_b, before, _))| id_a == id_b && after == before);
        if foldable {
            for (mine, theirs) in self.moves.iter_mut().zip(&next.moves) {
                mine.2 = theirs.2;
            }
            true
        } else {
            false
        }
    }
}

/// R1234 §5.52 — a reversible comment-frame **geometry** edit. Two shapes share
/// it: a *move* translates the frame's rect AND carries every node it contained
/// by the same delta (the Blueprint "drag the comment box, its contents come
/// along" contract); a *resize* changes only `w`/`h` with an empty `moves` (the
/// nodes stay put and the membership is recomputed lazily by
/// [`CommentFrame::contains_node`]). Stores the frame's `before` / `after` rect
/// plus one [`NodeMove`] per translated member, so undo / redo is an
/// O(members) reposition, never a graph-wide delta ([[granular-undo-not-snapshot]]).
/// Like [`RenameCmd`] (and unlike [`MoveNodesCmd`]) it does NOT coalesce — an
/// `intervene frame.<id>.x` is a discrete RPC edit, not a keyboard nudge burst,
/// so every committed geometry edit is its own undo step.
struct FrameGeomCmd {
    frames: Rc<Signal<Vec<CommentFrame>>>,
    nodes: Rc<Signal<Vec<GraphNode>>>,
    id: FrameId,
    /// The frame's `(x, y, w, h)` before / after this edit.
    before: (i32, i32, i32, i32),
    after: (i32, i32, i32, i32),
    /// `(id, before, after)` per member node the move carried — empty for a
    /// resize (which never moves nodes).
    moves: Vec<NodeMove>,
    /// "Move frame" / "Resize frame" — set by the caller so the undo label
    /// names the gesture.
    label: Cow<'static, str>,
}

impl FrameGeomCmd {
    /// Set the frame's rect + every member node to its `after` (`to_after`) or
    /// `before` position, one write per touched signal (the node write is
    /// skipped for a resize — an empty `moves`). An absent frame / member is
    /// skipped: a LIFO undo cannot reach this edit while the entity is deleted,
    /// but the write stays total ([`MoveNodesCmd::set_all`] discipline).
    fn set_geom(&self, to_after: bool) {
        let rect = if to_after { self.after } else { self.before };
        self.frames.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(f) = next.iter_mut().find(|f| f.id == self.id) {
                (f.x, f.y, f.w, f.h) = rect;
            }
            next
        });
        if !self.moves.is_empty() {
            self.nodes.set_with(|prev| {
                let mut next = prev.clone();
                for (id, before, after) in &self.moves {
                    let pos = if to_after { after } else { before };
                    if let Some(n) = next.iter_mut().find(|n| n.id == *id) {
                        n.x = pos.0;
                        n.y = pos.1;
                    }
                }
                next
            });
        }
    }
}

impl UndoCommand for FrameGeomCmd {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.set_geom(true);
    }

    fn undo(&self) {
        self.set_geom(false);
    }
}

/// R1234 — the feasible one-axis translation `dx` of comment frame `frame`
/// keeping its origin AND every member node on the world surface: the requested
/// delta clamped to an interval `[lo, hi]` that always contains `0` (the current
/// state is already on-world). A *rigid* group clamp — the frame and its
/// contents move by the SAME delta, so their relative geometry is preserved even
/// at the world edge (a node can never slide out of the frame it sits in). With
/// no members only the frame origin bounds it, so an empty frame still cannot be
/// pushed off the world.
fn clamp_frame_dx(frame: &CommentFrame, members: &[GraphNode], dx: i32) -> i32 {
    let mut lo = -frame.x;
    let mut hi = WORLD - frame.x;
    for n in members {
        lo = lo.max(-n.x);
        hi = hi.min(WORLD - NODE_W - n.x);
    }
    dx.clamp(lo, hi)
}

/// R1234 — the vertical twin of [`clamp_frame_dx`]: the feasible `dy` keeping
/// the frame origin and every member within the world's `y` extent (the same
/// bound [`clamp_node_y`] enforces per node).
fn clamp_frame_dy(frame: &CommentFrame, members: &[GraphNode], dy: i32) -> i32 {
    let mut lo = -frame.y;
    let mut hi = WORLD - frame.y;
    for n in members {
        lo = lo.max(-n.y);
        hi = hi.min((WORLD - HEADER_H - 8) - n.y);
    }
    dy.clamp(lo, hi)
}

/// R1232 §5.52 — a titled, id-addressed graph entity a rename undo command
/// edits. [`GraphNode`] + [`CommentFrame`] are the two consumers: the arrival of
/// the 2nd unified [`RenameCmd`]/[`apply_rename`] over what were byte-identical
/// `RenameNodeCmd` + `RenameFrameCmd` copies ([[abstraction-needs-second-consumer]]).
/// `Serialize`/`Deserialize` are the `Signal<Vec<T>>` write bound.
trait Titled: Clone + PartialEq + Serialize + serde::de::DeserializeOwned + 'static {
    type Id: PartialEq + Copy;
    /// The entity kind for the undo label (`"node"` / `"frame"`).
    const KIND: &'static str;
    fn entity_id(&self) -> Self::Id;
    fn title_ref(&self) -> &str;
    fn set_title(&mut self, title: String);
}

impl Titled for GraphNode {
    type Id = NodeId;
    const KIND: &'static str = "node";
    fn entity_id(&self) -> NodeId {
        self.id
    }
    fn title_ref(&self) -> &str {
        &self.title
    }
    fn set_title(&mut self, title: String) {
        self.title = title;
    }
}

impl Titled for CommentFrame {
    type Id = FrameId;
    const KIND: &'static str = "frame";
    fn entity_id(&self) -> FrameId {
        self.id
    }
    fn title_ref(&self) -> &str {
        &self.title
    }
    fn set_title(&mut self, title: String) {
        self.title = title;
    }
}

/// R1232 §5.52 — a reversible **rename** of a [`Titled`] entity by id (the
/// generic that unified R878 `RenameNodeCmd` + R1227 `RenameFrameCmd` — those
/// were structurally identical, only the entity type differing, so the 2nd
/// consumer lifts them). Stores only the `before` / `after` title, so undo /
/// redo is an O(1) field write, never a graph-wide delta
/// ([[granular-undo-not-snapshot]]). A rename commits once per editing session
/// (Enter / blur), so it does NOT opt into the coalescing hook — every committed
/// rename is its own undo step (Qt's `QUndoStack` rename model).
struct RenameCmd<T: Titled> {
    entities: Rc<Signal<Vec<T>>>,
    id: T::Id,
    before: String,
    after: String,
}

impl<T: Titled> RenameCmd<T> {
    /// Set the renamed entity's title. A no-op if it is absent (a LIFO undo can
    /// never reach a rename while the entity is deleted, but the signal write
    /// stays total either way — the [`MoveNodesCmd::set_all`] discipline).
    fn set_title(&self, title: &str) {
        self.entities.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(e) = next.iter_mut().find(|e| e.entity_id() == self.id) {
                e.set_title(title.to_owned());
            }
            next
        });
    }
}

impl<T: Titled> UndoCommand for RenameCmd<T> {
    fn label(&self) -> Cow<'static, str> {
        Cow::Owned(format!("Rename {}", T::KIND))
    }

    fn redo(&self) {
        self.set_title(&self.after);
    }

    fn undo(&self) {
        self.set_title(&self.before);
    }
}

/// R1232 — rename [`Titled`] entity `id` undoably (the ONE rename path for both
/// nodes AND frames): trim, reject an empty / whitespace title or an unknown id
/// (graph unchanged), no-op (and journal nothing) when the trimmed title already
/// matches. The interactive commit (Enter / blur via [`commit_edit`]) and the
/// AI-first `intervene <node|frame>.<id>.title` both land here, so they cannot drift.
fn apply_rename<T: Titled>(
    entities: &Rc<Signal<Vec<T>>>,
    undo: &UndoStack,
    id: T::Id,
    title: &str,
) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(before) = entities
        .get()
        .iter()
        .find(|e| e.entity_id() == id)
        .map(|e| e.title_ref().to_owned())
    else {
        return false;
    };
    if before == trimmed {
        return true;
    }
    let cmd = RenameCmd {
        entities: Rc::clone(entities),
        id,
        before,
        after: trimmed.to_owned(),
    };
    // R1234 — `record` applies the fresh command's forward effect then journals
    // it; the pre-R1234 `cmd.redo(); push_applied(cmd)` hand-rolled exactly that
    // ([[use-substrate-not-hand-rolled-equivalent]]). `push_applied` is only for
    // an edit already applied out-of-band (the drag / keystroke path), not here.
    undo.record(cmd);
    true
}

/// R899 — one reversible edit to an input port's literal default value (a
/// per-field, non-coalescing undo step: each committed default change is its own
/// step). Stores the typed [`CellValue`] before / after, so undo / redo restore
/// the exact value.
///
/// R900 / R1232 — this is one of four node-editor [`UndoCommand`]s ([`GraphEdit`]
/// / [`MoveNodesCmd`] / [`RenameCmd`] / this). Each owns the shape of the one
/// mutation it reverses; a single closure-parameterised `FieldUndoCmd<T>` over
/// ALL of them would be a behaviour-bifurcating wrong abstraction (R853) —
/// `MoveNodesCmd` is multi-target + coalescing, this no-ops on a
/// total-order-equal value. The rename command is the one that DID generalise:
/// its node + frame copies were byte-identical, so R1232 lifted them to the
/// generic [`RenameCmd<T>`](RenameCmd) (the 2nd-consumer rule), whereas Move /
/// SetPortDefault genuinely differ and stay distinct.
struct SetPortDefaultCmd {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    id: NodeId,
    port: usize,
    before: CellValue,
    after: CellValue,
}

impl SetPortDefaultCmd {
    /// Write the port's default. A no-op if the node / port is absent (a LIFO
    /// undo cannot reach it while deleted, but the write stays total — the
    /// [`RenameCmd::set_title`] discipline).
    fn set_default(&self, value: &CellValue) {
        self.nodes.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(slot) = next
                .iter_mut()
                .find(|n| n.id == self.id)
                .and_then(|n| n.input_defaults.get_mut(self.port))
            {
                *slot = value.clone();
            }
            next
        });
    }
}

impl UndoCommand for SetPortDefaultCmd {
    fn label(&self) -> Cow<'static, str> {
        Cow::Borrowed("Set port default")
    }

    fn redo(&self) {
        self.set_default(&self.after);
    }

    fn undo(&self) {
        self.set_default(&self.before);
    }
}

/// R899 — apply an input-port default change undoably (the [`apply_rename`]
/// peer): reject an unknown id / out-of-range port (graph unchanged, `false`),
/// no-op (journal nothing) when the value is unchanged, else journal a
/// [`SetPortDefaultCmd`]. The ONE port-default mutation path — the AI-first
/// `intervene node.<id>.input_default.<port>` and the R901 interactive inline
/// editor (via [`apply_edit_commit`]) both land here, so they cannot drift.
/// The caller has already typed the value (an `intervene` against the port's
/// kind via [`CellValue::with_intervene`], the editor via `CellKind::parse`),
/// so a wrong type never reaches the journal.
fn apply_set_default(
    nodes: &Rc<Signal<Vec<GraphNode>>>,
    undo: &UndoStack,
    id: NodeId,
    port: usize,
    value: CellValue,
) -> bool {
    let Some(before) = nodes
        .get()
        .into_iter()
        .find(|n| n.id == id)
        .and_then(|n| n.input_defaults.get(port).cloned())
    else {
        return false;
    };
    // R900 / R920 — the no-op guard compares by the substrate's NaN-safe value
    // equality (`CellValue::value_eq`, built on the TOTAL order), not the derived
    // IEEE `PartialEq`: a `Float` default of `NaN` is `!= NaN` under `==`, so
    // re-setting it would journal a spurious second undo step. The one home for
    // "same typed value (NaN-safe)", shared with the property grid's modified
    // check (R920 lift).
    if before.value_eq(&value) {
        return true;
    }
    let cmd = SetPortDefaultCmd {
        nodes: Rc::clone(nodes),
        id,
        port,
        before,
        after: value,
    };
    // R1234 — `record` (apply-then-journal) over the hand-rolled `cmd.redo();
    // push_applied(cmd)` ([[use-substrate-not-hand-rolled-equivalent]]); the
    // value was NOT applied out-of-band, so this is a `record`, not a
    // `push_applied` (which is for the eager drag / keystroke path).
    undo.record(cmd);
    true
}

/// R853 / R918 — clamp `(x, y)` into the world bounds and write node `id`'s
/// position, returning `(before, after)` window positions (`None` for an absent
/// id). The non-journaling reposition primitive shared by `set_node_pos`
/// (the capture drag, called once per frame) and [`apply_set_pos`] (the
/// journaling single-move funnel) — the ONE place a node's position is clamped
/// and written.
fn set_pos_clamped(
    nodes: &Rc<Signal<Vec<GraphNode>>>,
    id: NodeId,
    x: i32,
    y: i32,
) -> Option<((i32, i32), (i32, i32))> {
    let mut result = None;
    nodes.set_with(|prev| {
        let mut next = prev.clone();
        if let Some(node) = next.iter_mut().find(|n| n.id == id) {
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
    nodes: &Rc<Signal<Vec<GraphNode>>>,
    members: &[(NodeId, f64, f64, i32, i32)],
    gx: f64,
    gy: f64,
) {
    for &(id, grab_dx, grab_dy, _, _) in members {
        set_pos_clamped(nodes, id, round_i32(gx + grab_dx), round_i32(gy + grab_dy));
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
/// one *coalescable* [`MoveNodesCmd`], so a Details-panel `x` edit then `y` edit
/// (or an arrow-nudge burst) fold into a single undo step. An unchanged position
/// journals nothing (the [`apply_rename`] / [`apply_set_default`] no-op
/// discipline). The ONE position-commit funnel the panel's PosX/PosY inline
/// editor and the `intervene node.<id>.{x,y}` arm share, so a panel edit and an
/// RPC move are one undoable mutation path ([[setter-wire-returns-read-outcome]]).
fn apply_set_pos(
    nodes: &Rc<Signal<Vec<GraphNode>>>,
    undo: &UndoStack,
    id: NodeId,
    x: i32,
    y: i32,
) -> bool {
    let Some((before, after)) = set_pos_clamped(nodes, id, x, y) else {
        return false;
    };
    if before != after {
        undo.push_applied(MoveNodesCmd {
            nodes: Rc::clone(nodes),
            moves: vec![(id, before, after)],
            coalescable: true,
        });
    }
    true
}

/// R901 — the [`CellKind`] of node `node`'s input port `port`'s default, or
/// `None` when the node / port is absent. Drives the inline editor's keystroke
/// gate (a `Float` accepts digits / sign / `.`, a `Color` hex digits + `#`)
/// and its commit parse — the one place a port default's editor type is read.
fn port_default_kind(nodes: &[GraphNode], node: NodeId, port: usize) -> Option<CellKind> {
    nodes
        .iter()
        .find(|n| n.id == node)?
        .input_default(port)
        .map(CellValue::kind)
}

/// R901 — commit inline-editor `text` into `target` through the matching
/// field SSOT: a title routes to [`apply_rename`] (trim / reject-empty), a
/// port default parses by the port's [`CellKind`] and routes to
/// [`apply_set_default`] (a malformed numeric / hex keeps the prior value — no
/// data loss, the `CellKind::parse` contract). The ONE place an inline commit
/// dispatches by target, shared by the keyboard / blur [`commit_edit`] and the
/// begin-edit migration (committing a different in-flight target before
/// opening a new one), so the two can never drift.
fn apply_edit_commit(
    nodes: &Rc<Signal<Vec<GraphNode>>>,
    undo: &UndoStack,
    target: EditTarget,
    text: &str,
) {
    match target {
        EditTarget::Title(id) => {
            let _ = apply_rename(nodes, undo, id, text);
        }
        EditTarget::PortDefault { node, port } => {
            if let Some(value) =
                port_default_kind(&nodes.get(), node, port).and_then(|k| k.parse(text))
            {
                let _ = apply_set_default(nodes, undo, node, port, value);
            }
        }
        // R918 — a position edit parses the typed coordinate and routes to the
        // shared `apply_set_pos` funnel. A malformed value keeps the prior
        // position (no data loss — the `CellKind::Int` keystroke gate already
        // bars non-numeric input, this guards a lone `-` or empty field).
        EditTarget::PosX(id) => {
            if let (Ok(coord), Some(node)) = (
                text.trim().parse::<i32>(),
                nodes.get().into_iter().find(|n| n.id == id),
            ) {
                let _ = apply_set_pos(nodes, undo, id, coord, node.y);
            }
        }
        EditTarget::PosY(id) => {
            if let (Ok(coord), Some(node)) = (
                text.trim().parse::<i32>(),
                nodes.get().into_iter().find(|n| n.id == id),
            ) {
                let _ = apply_set_pos(nodes, undo, id, node.x, coord);
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
    /// R1227 — the comment-frame list ([`use_frames`]) + its monotonic id source
    /// ([`use_next_frame_id`]), bundled here with the other shared holders.
    frames: Rc<Signal<Vec<CommentFrame>>>,
    next_frame_id: Rc<Cell<u32>>,
}

/// The node-graph coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) plus the internal drag latches.
struct NodeGraphExternal {
    nodes: Rc<Signal<Vec<GraphNode>>>,
    edges: Rc<Signal<Vec<Edge>>>,
    /// R1227 — the comment-frame annotation list + its stable-id source.
    frames: Rc<Signal<Vec<CommentFrame>>>,
    next_frame_id: Rc<Cell<u32>>,
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
            frames: services.frames,
            next_frame_id: services.next_frame_id,
            selection,
            preview,
            next_edge_id,
            next_node_id,
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
            marquee: RefCell::new(None),
        }
    }

    fn node_count(&self) -> usize {
        self.nodes.get().len()
    }

    fn node_by_id(&self, id: NodeId) -> Option<GraphNode> {
        self.nodes.get().into_iter().find(|n| n.id == id)
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
            .map(|id| format!("node.{}.{field}", id.raw()))
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

    /// R877 — `frame_all` (`f`, the Unreal / Blender "frame" idiom): fit
    /// the node bounding box into the canvas with [`FRAME_MARGIN`],
    /// clamped to the zoom range, and centre it. `false` on an empty
    /// graph (nothing to frame, viewport unchanged).
    fn frame_all(&self) -> bool {
        let nodes = self.nodes.get();
        // R948 — the union bbox over all nodes (the [`node_bounds`] SSOT, also
        // the selection-bbox source for `align_selected`).
        let Some((min_x, min_y, max_x, max_y)) = node_bounds(nodes.iter()) else {
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
            frames: Rc::clone(&self.frames),
            selection: Rc::clone(&self.selection),
            label: label.into(),
            delta,
            sel_before,
            sel_after,
        });
    }

    /// R853/R879 — journal a (multi-)node move onto the undo stack as ONE
    /// step. The positions are already applied (the live drag / nudge /
    /// intervene set them), so this `push_applied`s the [`MoveNodesCmd`]
    /// without re-applying; the stack's coalescing folds a contiguous
    /// `coalescable` same-member run (a nudge burst) into one step.
    /// Unmoved members are dropped; an all-unmoved set journals nothing.
    fn record_moves(&self, mut moves: Vec<NodeMove>, coalescable: bool) {
        moves.retain(|(_, before, after)| before != after);
        if moves.is_empty() {
            return;
        }
        self.undo.push_applied(MoveNodesCmd {
            nodes: Rc::clone(&self.nodes),
            moves,
            coalescable,
        });
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
            frames: self.frames.get(),
            next_frame_id: self.next_frame_id.get(),
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
        self.frames.set(g.frames);
        self.next_frame_id.set(g.next_frame_id);
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
    /// record the `MoveNodeCmd` once the gesture / keystroke settles.
    fn set_node_pos(&self, id: NodeId, x: i32, y: i32) -> bool {
        set_pos_clamped(&self.nodes, id, x, y).is_some()
    }

    /// R849 — create a new node of [`PALETTE`] kind `kind` at the next cascade
    /// position, minting a fresh stable [`NodeId`] (monotonic, never reused),
    /// and select it. Returns the new id, or `None` for an out-of-range kind.
    /// The single mutation behind both a palette card click (`handle_send`)
    /// and the `add_node` RPC verb — the graph can finally *grow*, not only be
    /// rearranged. A new node has no edges, so no edge / selection bookkeeping
    /// is needed (the stable-id model: adding is purely additive).
    fn add_node(&self, kind: usize) -> Option<NodeId> {
        let &(title, input_ports, output_ports) = PALETTE.get(kind)?;
        let raw = self.next_node_id.get();
        self.next_node_id.set(raw + 1);
        let id = NodeId(raw);
        // Cascade in minted order from the spawn point so repeated adds fan out
        // instead of stacking exactly on one another. R877 — the spawn point is
        // a fixed *canvas* position projected into graph space through the
        // current viewport, so a new node always lands in the visible view
        // (spawning at a fixed graph point would drop it off-screen once the
        // canvas has panned away).
        let step = i32::try_from(raw.saturating_sub(first_dynamic_node_id())).unwrap_or(0) % 8;
        let (gx, gy) = self.cursor_graph(
            f64::from(SPAWN_X) / f64::from(WIN_W),
            f64::from(SPAWN_Y) / f64::from(WIN_H),
        );
        let x = clamp_node_x(round_i32(gx) + step * SPAWN_STEP);
        let y = clamp_node_y(round_i32(gy) + step * SPAWN_STEP);
        let node = GraphNode {
            id,
            title: title.to_owned(),
            x,
            y,
            input_ports: input_ports.to_vec(),
            output_ports: output_ports.to_vec(),
            input_defaults: input_ports.iter().map(|t| t.default_value()).collect(),
        };
        let sel_before = self.selection.get();
        // `record` applies the edit forward — pushing the node and selecting it
        // (the prior direct writes) — so a single Ctrl+Z removes it again.
        self.record_edit(
            format!("Add {title}"),
            GraphDelta {
                added_nodes: vec![node],
                ..GraphDelta::default()
            },
            sel_before,
            Selection::single(id),
        );
        Some(id)
    }

    // ── R1220 pin-drop create menu (drag off a pin → typed menu → auto-wire) ──

    /// The source output's [`PortType`] + the current candidate [`PALETTE`]
    /// indices for the open menu `pc` (its `filter` applied). `None` if the
    /// source pin has gone invalid (an undo removed its node while the menu was
    /// open) — every menu path re-derives from here, so a mid-menu delete never
    /// commits against a stale pin.
    fn pin_candidates(&self, pc: &PinCreate) -> Option<(PortType, Vec<usize>)> {
        let node = self.node_by_id(pc.from_node)?;
        let from_ty = node.output_type(pc.from_port)?;
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
        let Some(node) = self.node_by_id(from_node) else {
            return false;
        };
        let Some(from_ty) = node.output_type(from_port) else {
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
        let &(title, input_ports, output_ports) = PALETTE.get(kind)?;
        let raw = self.next_node_id.get();
        self.next_node_id.set(raw + 1);
        let node_id = NodeId(raw);
        let node = GraphNode {
            id: node_id,
            title: title.to_owned(),
            x: pc.at_graph.0,
            y: pc.at_graph.1,
            input_ports: input_ports.to_vec(),
            output_ports: output_ports.to_vec(),
            input_defaults: input_ports.iter().map(|t| t.default_value()).collect(),
        };
        let edge_id = EdgeId(self.next_edge_id.get());
        self.next_edge_id.set(edge_id.raw() + 1);
        let edge = Edge {
            id: edge_id,
            from_node: pc.from_node,
            from_port: pc.from_port,
            to_node: node_id,
            to_port: target_port,
        };
        let sel_before = self.selection.get();
        self.record_edit(
            format!("Add {title} + wire"),
            GraphDelta {
                added_nodes: vec![node],
                added_edges: vec![edge],
                ..GraphDelta::default()
            },
            sel_before,
            Selection::single(node_id),
        );
        self.pin_create.set(None);
        Some(node_id)
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
            "from_node": pc.from_node.raw(),
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
        let nodes = self.nodes.get();
        let src = nodes.iter().find(|n| n.id == from_node)?;
        let dst = nodes.iter().find(|n| n.id == to_node)?;
        // Out-of-range port = a missing typed socket (the pre-R898 arity reject).
        let (from_ty, to_ty) = (src.output_type(from_port)?, dst.input_type(to_port)?);
        // R898 — typed-port validation: the output's type must be assignable to
        // the destination input's type (exact, or a `Float`->`Vector` scalar
        // broadcast). A material / blueprint graph rejects an ill-typed wire.
        if !from_ty.is_assignable_to(to_ty) {
            return None;
        }
        let edges = self.edges.get();
        let dup = edges.iter().any(|e| {
            Some(e.id) != ignore
                && e.from_node == from_node
                && e.from_port == from_port
                && e.to_node == to_node
                && e.to_port == to_port
        });
        if dup {
            return None;
        }
        // Input single-wire rule: the new wire displaces any existing wire into
        // the same target input — captured as a removed delta so undo restores it.
        Some(
            edges
                .iter()
                .copied()
                .filter(|e| Some(e.id) != ignore && e.to_node == to_node && e.to_port == to_port)
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
    /// R1235 — mint the next stable [`EdgeId`], bumping the monotonic counter.
    /// The one id source shared by [`commit_new_edge`](Self::commit_new_edge)
    /// (connect / reconnect) and the reroute splice ([`add_reroute`](Self::add_reroute),
    /// which mints two) — a deleted-then-saved id is never re-handed-out
    /// (the counter only advances), so a splice + undo leaves no id collision.
    fn mint_edge_id(&self) -> EdgeId {
        let id = EdgeId(self.next_edge_id.get());
        self.next_edge_id.set(id.raw() + 1);
        id
    }

    fn commit_new_edge(
        &self,
        label: &'static str,
        from_node: NodeId,
        from_port: usize,
        to_node: NodeId,
        to_port: usize,
        removed: Vec<Edge>,
    ) {
        let id = self.mint_edge_id();
        let new_edge = Edge {
            id,
            from_node,
            from_port,
            to_node,
            to_port,
        };
        let sel_before = self.selection.get();
        let removed_ids: Vec<EdgeId> = removed.iter().map(|e| e.id).collect();
        let sel_after = validate_after(sel_before.clone(), &[], &removed_ids);
        self.record_edit(
            label,
            GraphDelta {
                added_edges: vec![new_edge],
                removed_edges: removed,
                ..GraphDelta::default()
            },
            sel_before,
            sel_after,
        );
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
        self.commit_new_edge("Connect", from_node, from_port, to_node, to_port, replaced);
        true
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
    /// new connection, consistent with the remove+add [`GraphDelta`] model.
    fn reconnect_edge(&self, edge_id: EdgeId, new_to_node: NodeId, new_to_port: usize) -> bool {
        let Some(old) = self.edges.get().iter().copied().find(|e| e.id == edge_id) else {
            return false;
        };
        if old.to_node == new_to_node && old.to_port == new_to_port {
            return true; // dropped back on its own input — nothing changed.
        }
        let Some(replaced) = self.validate_connection(
            old.from_node,
            old.from_port,
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
            old.from_node,
            old.from_port,
            new_to_node,
            new_to_port,
            removed,
        );
        true
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
        let edge = self.edges.get().iter().copied().find(|e| e.id == edge_id)?;
        let nodes = self.nodes.get();
        // The wire's type is its SOURCE output's type; both reroute ports take it.
        let ty = node_ref(&nodes, edge.from_node)?.output_type(edge.from_port)?;
        // Centre the reroute on the wire's straight midpoint (graph units — the
        // same space `edge_endpoints` + node positions live in).
        let (from, to) = edge_endpoints(&nodes, &edge)?;
        let mid_x = i32::midpoint(from.0, to.0);
        let mid_y = i32::midpoint(from.1, to.1);
        let node_raw = self.next_node_id.get();
        self.next_node_id.set(node_raw + 1);
        let mut reroute = GraphNode::new(node_raw, "Reroute", 0, 0, &[ty], &[ty]);
        reroute.x = clamp_node_x(mid_x - NODE_W / 2);
        reroute.y = clamp_node_y(mid_y - reroute.height() / 2);
        let node_id = reroute.id;
        // Mint the two replacement edges (A -> R, R -> B).
        let e_in = self.mint_edge_id();
        let e_out = self.mint_edge_id();
        let a_to_r = Edge {
            id: e_in,
            from_node: edge.from_node,
            from_port: edge.from_port,
            to_node: node_id,
            to_port: 0,
        };
        let r_to_b = Edge {
            id: e_out,
            from_node: node_id,
            from_port: 0,
            to_node: edge.to_node,
            to_port: edge.to_port,
        };
        let sel_before = self.selection.get();
        self.record_edit(
            "Insert reroute",
            GraphDelta {
                added_nodes: vec![reroute],
                added_edges: vec![a_to_r, r_to_b],
                removed_edges: vec![edge],
                ..GraphDelta::default()
            },
            sel_before,
            Selection::single(node_id),
        );
        Some(node_id)
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
            Some(n) => IntrospectValue::Int(i64::from(n.raw())),
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

    /// R899 — the `intervene node.<id>.input_default.<port>` write path. The
    /// value is type-checked against the port's kind by
    /// [`CellValue::with_intervene`] (a `Float` takes a float, a `Vector`/`Color`
    /// a `#RRGGBB[AA]` hex), then routed through the `apply_set_default` SSOT, so
    /// the AI write journals the same undoable [`SetPortDefaultCmd`] an inline
    /// editor would. An unknown field / out-of-range port is an `UnknownPath`.
    fn intervene_input_default(
        &mut self,
        id: NodeId,
        node: &GraphNode,
        field: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        let Some(port) = field
            .strip_prefix("input_default.")
            .and_then(|p| p.parse::<usize>().ok())
        else {
            return Err(InterveneError::UnknownPath);
        };
        let Some(current) = node.input_default(port) else {
            return Err(InterveneError::UnknownPath);
        };
        let next = current.with_intervene(value)?;
        // R900 — gate the result like the `apply_rename` caller does (symmetry):
        // the port existence is pre-checked above so `false` is currently
        // unreachable, but threading the funnel's success keeps the contract
        // explicit and total rather than silently swallowing a future failure.
        if apply_set_default(&self.nodes, &self.undo, id, port, next) {
            Ok(())
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    /// Remove the edge with stable id `id` (no-op + `false` if absent). R851 —
    /// the edge is stored as a removed delta so undo re-inserts it verbatim.
    fn remove_edge(&self, id: EdgeId) -> bool {
        let Some(edge) = self.edges.get().iter().copied().find(|e| e.id == id) else {
            return false;
        };
        let sel_before = self.selection.get();
        let sel_after = validate_after(sel_before.clone(), &[], &[id]);
        self.record_edit(
            "Disconnect",
            GraphDelta {
                removed_edges: vec![edge],
                ..GraphDelta::default()
            },
            sel_before,
            sel_after,
        );
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
        let nodes = self.nodes.get();
        let cut: Vec<Edge> = self
            .edges
            .get()
            .iter()
            .copied()
            .filter(|e| {
                edge_endpoints(&nodes, e)
                    .is_some_and(|(from, to)| edge_crosses_segment(from, to, a, b))
            })
            .collect();
        if cut.is_empty() {
            return Vec::new();
        }
        let ids: Vec<EdgeId> = cut.iter().map(|e| e.id).collect();
        let sel_before = self.selection.get();
        let sel_after = validate_after(sel_before.clone(), &[], &ids);
        self.record_edit(
            "Cut wires",
            GraphDelta {
                removed_edges: cut,
                ..GraphDelta::default()
            },
            sel_before,
            sel_after,
        );
        ids
    }

    /// R1226 — the `cut_wires` invoke arm, extracted so the `invoke` match stays
    /// within the workspace `too_many_lines` ceiling. Parses `"x1,y1,x2,y2"` and
    /// returns the CSV of cut edge ids; a malformed spec Rejects, a non-string
    /// arg is a TypeMismatch.
    fn invoke_cut_wires(&self, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (a, b) = parse_cut_spec(&s).ok_or(InvokeError::Rejected)?;
        Ok(IntrospectValue::Text(csv_ids(
            self.cut_wires(a, b).iter().map(|id| id.raw()),
        )))
    }

    // ─── R1227 comment frames ─────────────────────────────────────────

    fn frame_by_id(&self, id: FrameId) -> Option<CommentFrame> {
        self.frames.get().into_iter().find(|f| f.id == id)
    }

    /// R1227 — frame the current node selection: the bounding box of the
    /// selected nodes grown by [`FRAME_PAD`] on every side (plus
    /// [`FRAME_HEADER_H`] on top for the title strip), titled `"Comment N"`. The
    /// canonical Blueprint "comment the selection" create (the RPC `add_frame`
    /// verb + the future `C` gesture funnel here). Undoable
    /// (`GraphDelta.added_frames`). `None` when no node is selected (a frame with
    /// nothing to annotate is not created). The node selection is untouched —
    /// frames are a separate annotation axis, not part of [`Selection`].
    fn add_frame(&self) -> Option<FrameId> {
        let sel = self.selection.get().nodes();
        let nodes = self.nodes.get();
        // R1230 — the selection's bounding box through the `node_bounds` SSOT
        // (the same fold `align_selected` uses); the pre-R1230 hand-rolled fold
        // here bypassed both `node_bounds` and the `bottom()` accessor.
        let (left, top, right, bottom) = node_bounds(nodes.iter().filter(|n| sel.contains(&n.id)))?;
        let x = (left - FRAME_PAD).max(0);
        let y = (top - FRAME_PAD - FRAME_HEADER_H).max(0);
        let raw = self.next_frame_id.get();
        self.next_frame_id.set(raw + 1);
        let id = FrameId(raw);
        let frame = CommentFrame {
            id,
            x,
            y,
            w: (right + FRAME_PAD) - x,
            h: (bottom + FRAME_PAD) - y,
            title: format!("Comment {}", raw + 1),
        };
        let sel = self.selection.get();
        self.record_edit(
            "Add frame",
            GraphDelta {
                added_frames: vec![frame],
                ..GraphDelta::default()
            },
            sel.clone(),
            sel,
        );
        Some(id)
    }

    /// R1227 — remove comment frame `id` (the nodes it annotated stay). Undoable
    /// (`GraphDelta.removed_frames` — one Ctrl+Z restores it with its
    /// [`FrameId`] + title). `false` for an unknown id.
    fn remove_frame(&self, id: FrameId) -> bool {
        let Some(frame) = self.frame_by_id(id) else {
            return false;
        };
        let sel = self.selection.get();
        self.record_edit(
            "Remove frame",
            GraphDelta {
                removed_frames: vec![frame],
                ..GraphDelta::default()
            },
            sel.clone(),
            sel,
        );
        true
    }

    /// R1234 — move comment frame `id` to a new `x` (`new_x`) and / or `y`
    /// (`new_y`), carrying every node it CURRENTLY contains by the same clamped
    /// delta as ONE undo step ([`FrameGeomCmd`], label "Move frame"). The
    /// Blueprint move-with-contents contract — the membership is snapshotted at
    /// the start of the move ([`CommentFrame::contains_node`]), so the moved set
    /// is exactly what the paint + the `contains` query show. Nodes outside the
    /// frame are untouched. `false` for an unknown id.
    fn translate_frame(&self, id: FrameId, new_x: Option<i32>, new_y: Option<i32>) -> bool {
        let Some(frame) = self.frame_by_id(id) else {
            return false;
        };
        let members: Vec<GraphNode> = self
            .nodes
            .get()
            .iter()
            .filter(|n| frame.contains_node(n))
            .cloned()
            .collect();
        let dx = clamp_frame_dx(&frame, &members, new_x.map_or(0, |x| x - frame.x));
        let dy = clamp_frame_dy(&frame, &members, new_y.map_or(0, |y| y - frame.y));
        let after = (frame.x + dx, frame.y + dy, frame.w, frame.h);
        let moves: Vec<NodeMove> = members
            .iter()
            .map(|n| (n.id, (n.x, n.y), (n.x + dx, n.y + dy)))
            .collect();
        self.commit_frame_geom(
            id,
            (frame.x, frame.y, frame.w, frame.h),
            after,
            moves,
            "Move frame",
        )
    }

    /// R1234 — resize comment frame `id` to a new `w` (`new_w`) and / or `h`
    /// (`new_h`), clamped to `[FRAME_MIN, WORLD - origin]` so the box keeps its
    /// chrome and its right / bottom edge stays on the world surface. The origin
    /// is fixed and NO node moves: a resize changes which nodes the frame
    /// contains (recomputed lazily by [`CommentFrame::contains_node`]), it does
    /// not drag them ([`FrameGeomCmd`], label "Resize frame"). `false` for an
    /// unknown id.
    fn resize_frame(&self, id: FrameId, new_w: Option<i32>, new_h: Option<i32>) -> bool {
        let Some(frame) = self.frame_by_id(id) else {
            return false;
        };
        let w = new_w.map_or(frame.w, |w| {
            w.clamp(FRAME_MIN, (WORLD - frame.x).max(FRAME_MIN))
        });
        let h = new_h.map_or(frame.h, |h| {
            h.clamp(FRAME_MIN, (WORLD - frame.y).max(FRAME_MIN))
        });
        self.commit_frame_geom(
            id,
            (frame.x, frame.y, frame.w, frame.h),
            (frame.x, frame.y, w, h),
            Vec::new(),
            "Resize frame",
        )
    }

    /// R1234 — the ONE commit funnel [`Self::translate_frame`] + [`Self::resize_frame`]
    /// share: build the geometry edit and hand it to [`UndoStack::record`],
    /// which applies its forward effect then journals it as a single
    /// [`FrameGeomCmd`] (the same primitive [`apply_rename`] routes through — NOT
    /// a hand-rolled `cmd.redo(); push_applied`). A no-op edit (rect unchanged,
    /// every move stationary) journals nothing. Always `true` — the frame was
    /// validated at the call site.
    fn commit_frame_geom(
        &self,
        id: FrameId,
        before: (i32, i32, i32, i32),
        after: (i32, i32, i32, i32),
        moves: Vec<NodeMove>,
        label: impl Into<Cow<'static, str>>,
    ) -> bool {
        if before == after && moves.iter().all(|(_, b, a)| b == a) {
            return true;
        }
        self.undo.record(FrameGeomCmd {
            frames: Rc::clone(&self.frames),
            nodes: Rc::clone(&self.nodes),
            id,
            before,
            after,
            moves,
            label: label.into(),
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
            .position(|&(name, _, _)| name == *s)
            .ok_or(InvokeError::Rejected)?;
        let id = self.add_node(kind).ok_or(InvokeError::Rejected)?;
        Ok(IntrospectValue::Int(i64::from(id.raw())))
    }

    /// R1227 — the `add_frame` verb arm: frame the selection, returning the new
    /// id (`Null` when nothing is selected).
    fn invoke_add_frame(&mut self) -> IntrospectValue {
        match self.add_frame() {
            Some(id) => IntrospectValue::Int(i64::from(id.raw())),
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
        Ok(IntrospectValue::Bool(self.remove_frame(FrameId(id))))
    }

    /// R1227 — the `frame.<id>.<field>` read: the comment-frame rect / title +
    /// `contains` (the CSV of node ids whose centre lies inside). `None` when the
    /// path is not a frame path or the id / field is unknown.
    fn query_frame(&self, path: &str) -> Option<IntrospectValue> {
        let rest = path.strip_prefix("frame.")?;
        let (id_str, field) = rest.split_once('.')?;
        let id = FrameId(id_str.parse().ok()?);
        let frames = self.frames.get();
        let f = frames.iter().find(|f| f.id == id)?;
        match field {
            "title" => Some(IntrospectValue::Text(f.title.clone())),
            "x" => Some(IntrospectValue::Int(i64::from(f.x))),
            "y" => Some(IntrospectValue::Int(i64::from(f.y))),
            "w" => Some(IntrospectValue::Int(i64::from(f.w))),
            "h" => Some(IntrospectValue::Int(i64::from(f.h))),
            "contains" => Some(IntrospectValue::Text(csv_ids(
                self.nodes
                    .get()
                    .iter()
                    .filter(|n| f.contains_node(n))
                    .map(|n| n.id.raw()),
            ))),
            _ => None,
        }
    }

    /// R1227 / R1234 — the `intervene frame.<id>.<field>` write. `title` renames
    /// through the shared [`apply_rename`] SSOT (journaled). `x` / `y` MOVE the
    /// frame + the nodes it contains ([`Self::translate_frame`] — move-with-contents);
    /// `w` / `h` RESIZE the box, origin fixed, nodes untouched ([`Self::resize_frame`]).
    /// Each rect write journals one [`FrameGeomCmd`]. An unknown id is caught up
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
        let id = FrameId(id_str.parse().map_err(|_| InterveneError::UnknownPath)?);
        if self.frame_by_id(id).is_none() {
            return Err(InterveneError::UnknownPath);
        }
        match field {
            "title" => match value {
                IntrospectValue::Text(t) => {
                    if apply_rename(&self.frames, &self.undo, id, &t) {
                        Ok(())
                    } else {
                        Err(InterveneError::OutOfRange)
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
        let removed: Vec<GraphNode> = self
            .nodes
            .get()
            .iter()
            .filter(|n| ids.contains(&n.id))
            .cloned()
            .collect();
        if removed.is_empty() {
            return false;
        }
        let incident: Vec<Edge> = self
            .edges
            .get()
            .iter()
            .copied()
            .filter(|e| ids.contains(&e.from_node) || ids.contains(&e.to_node))
            .collect();
        let sel_before = self.selection.get();
        let removed_ids: Vec<NodeId> = removed.iter().map(|n| n.id).collect();
        let incident_ids: Vec<EdgeId> = incident.iter().map(|e| e.id).collect();
        let sel_after = validate_after(sel_before.clone(), &removed_ids, &incident_ids);
        let label: Cow<'static, str> = if removed.len() == 1 {
            Cow::Borrowed("Delete node")
        } else {
            Cow::Owned(format!("Delete {} nodes", removed.len()))
        };
        self.record_edit(
            label,
            GraphDelta {
                removed_nodes: removed,
                removed_edges: incident,
                ..GraphDelta::default()
            },
            sel_before,
            sel_after,
        );
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
    fn dissolve_node(&self, id: NodeId) -> bool {
        let edges = self.edges.get();
        let incoming: Vec<Edge> = edges.iter().copied().filter(|e| e.to_node == id).collect();
        let outgoing: Vec<Edge> = edges
            .iter()
            .copied()
            .filter(|e| e.from_node == id)
            .collect();
        // Exactly one hop in, one hop out — otherwise the bridge is ambiguous.
        let ([a_edge], [b_edge]) = (incoming.as_slice(), outgoing.as_slice()) else {
            return false;
        };
        let (a_node, a_port) = (a_edge.from_node, a_edge.from_port);
        let (b_node, b_port) = (b_edge.to_node, b_edge.to_port);
        // Removing the hop's two edges frees B's input, so the single-wire rule
        // holds by construction; the checks here are self-loop / type / exact
        // duplicate. (A reroute always passes — its ports carry the wire type.)
        if a_node == b_node {
            return false;
        }
        let nodes = self.nodes.get();
        let Some(ty_a) = node_ref(&nodes, a_node).and_then(|n| n.output_type(a_port)) else {
            return false;
        };
        let Some(ty_b) = node_ref(&nodes, b_node).and_then(|n| n.input_type(b_port)) else {
            return false;
        };
        if !ty_a.is_assignable_to(ty_b) {
            return false;
        }
        if edges.iter().any(|e| {
            e.from_node == a_node
                && e.from_port == a_port
                && e.to_node == b_node
                && e.to_port == b_port
        }) {
            return false;
        }
        let Some(removed_node) = node_ref(&nodes, id).cloned() else {
            return false;
        };
        let bridge = Edge {
            id: self.mint_edge_id(),
            from_node: a_node,
            from_port: a_port,
            to_node: b_node,
            to_port: b_port,
        };
        let sel_before = self.selection.get();
        let sel_after = validate_after(sel_before.clone(), &[id], &[a_edge.id, b_edge.id]);
        self.record_edit(
            "Dissolve node",
            GraphDelta {
                removed_nodes: vec![removed_node],
                removed_edges: vec![*a_edge, *b_edge],
                added_edges: vec![bridge],
                ..GraphDelta::default()
            },
            sel_before,
            sel_after,
        );
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

    /// Hit-test a window-px click against every wire; the first within
    /// tolerance is the selection candidate (its stable [`EdgeId`]).
    /// R877 — `(px, py)` in graph units (the caller converts the cursor via
    /// [`cursor_graph`](Self::cursor_graph)); the hit halo is screen-constant.
    fn hit_test_edge(&self, px: f64, py: f64) -> Option<EdgeId> {
        let nodes = self.nodes.get();
        let threshold = f64::from(EDGE_HIT_THRESHOLD) / self.zoom.get();
        self.edges
            .get()
            .iter()
            .find(|e| {
                edge_endpoints(&nodes, e)
                    .is_some_and(|(from, to)| point_near_edge(px, py, from, to, threshold))
            })
            .map(|e| e.id)
    }

    /// R948 — the selection move-loop SSOT behind nudge / align / distribute:
    /// map `target(n)` onto every node in `sel` through the clamped
    /// `set_node_pos`, inside one reactive [`batch`] (so subscribers see one
    /// atomic group move), and journal the net displacement as ONE
    /// [`MoveNodesCmd`]. `coalescable` folds a contiguous same-member run — true
    /// for an arrow-nudge burst, false for a discrete align / distribute
    /// command. Returns whether any node actually moved (a fully-clamped or
    /// already-in-place run journals nothing and returns `false`). The three
    /// callers differ only in how they compute `target` ([[three-site-internal-duplication-substrate-lift]]).
    fn apply_node_moves(
        &self,
        sel: &[GraphNode],
        coalescable: bool,
        target: impl Fn(&GraphNode) -> (i32, i32),
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
    fn selected_nodes(&self) -> Vec<GraphNode> {
        let members = self.selection.get().nodes();
        let nodes = self.nodes.get();
        nodes
            .iter()
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
            AlignSpec::CenterH => (i32::midpoint(left, right) - NODE_W / 2, n.y),
            AlignSpec::Right => (right - NODE_W, n.y),
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
                    DistributeAxis::Horizontal => (round_i32((c2 - f64::from(NODE_W)) / 2.0), n.y),
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

    /// Select a node by id (must exist). The sum type makes any prior edge
    /// selection vanish for free — no "clear the other" bookkeeping.
    /// R920 — write the selection, first committing any in-flight *panel* edit
    /// whose node is leaving the single selection. The Details panel renders only
    /// the single selected node, so a selection change that orphans a panel edit
    /// must end it (Unreal commit-on-selection-change, like a blur) — otherwise
    /// the shared field would paint nowhere while `query editing` still advertised
    /// it (an introspection lie reachable on the RPC path). A *card* edit is
    /// selection-independent (its card always paints), so it is left untouched.
    /// Every user-facing selection mutator routes through here.
    fn set_selection(&self, next: Selection) {
        if let Some(active) = self.editing.get() {
            if active.surface == EditSurface::Panel && next.node() != Some(active.target.node()) {
                let text = self.edit_buffer.text();
                apply_edit_commit(&self.nodes, &self.undo, active.target, &text);
                self.editing.set(None);
                self.edit_buffer.set_text(String::new());
            }
        }
        self.selection.set(next);
    }

    fn select_node(&self, id: Option<NodeId>) {
        let next = id
            .filter(|id| self.nodes.get().iter().any(|n| n.id == *id))
            .map_or(Selection::None, Selection::single);
        self.set_selection(next);
    }

    /// R879 — `Ctrl`-toggle `id`'s membership, leaving the rest of the node
    /// selection intact (a prior edge selection is replaced — the sum type's
    /// node-xor-edge rule). Removing the last member collapses to `None`
    /// through the construction funnel. Unknown ids are ignored.
    fn toggle_node(&self, id: NodeId) {
        if !self.nodes.get().iter().any(|n| n.id == id) {
            return;
        }
        let mut set = self.selection.get().nodes();
        toggle_member(&mut set, id);
        self.set_selection(Selection::from_nodes(set));
    }

    /// R879 — `Shift`-add `id` to the node selection (an unordered graph
    /// has no range to extend, so Shift means "add" — the Unreal graph
    /// convention). Unknown ids are ignored.
    fn add_node_to_selection(&self, id: NodeId) {
        if !self.nodes.get().iter().any(|n| n.id == id) {
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
        let all: BTreeSet<NodeId> = self.nodes.get().iter().map(|n| n.id).collect();
        if all.is_empty() {
            return false;
        }
        self.set_selection(Selection::from_nodes(all));
        true
    }

    /// R880 — apply a completed marquee: every node whose card intersects
    /// the graph-space rect joins the hit set (Qt rubber-band / Unreal
    /// intersects semantics — touching counts). The release modifiers pick
    /// the area form of the R879 click policy through the framework
    /// [`SelectionChord`] decode (R880.1 — *extend* on an unordered canvas
    /// means union): plain *replaces* the selection with the hit
    /// set (an empty sweep clears — the background-click deselect
    /// generalised to an area), `Ctrl` *toggles* each hit member, `Shift`
    /// *unions* the hit set in.
    fn apply_marquee(&self, rect: MarqueeRect, mods: Modifiers) {
        let (x0, y0, x1, y1) = rect;
        let hit: BTreeSet<NodeId> = self
            .nodes
            .get()
            .iter()
            .filter(|n| n.x <= x1 && n.right() >= x0 && n.y <= y1 && n.bottom() >= y0)
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
            if !self.nodes.get().iter().any(|n| n.id == id) {
                return Err(InterveneError::OutOfRange);
            }
            members.insert(id);
        }
        self.set_selection(Selection::from_nodes(members));
        Ok(())
    }

    /// Select an edge by id (must exist).
    fn select_edge(&self, id: Option<EdgeId>) {
        let next = id
            .filter(|id| self.edges.get().iter().any(|e| e.id == *id))
            .map_or(Selection::None, Selection::Edge);
        self.set_selection(next);
    }

    /// R878 / R901 — begin an inline edit of `target` (a node title or an
    /// input port default): validate the target exists, commit any in-flight
    /// edit of a *different* target first (the Qt item-view discipline — an
    /// open editor commits when another item enters edit; without it the
    /// migration would silently discard the typed text), flag
    /// [`use_active_edit`], seed the shared field with the target's current
    /// text (caret parked at the end — the todomvc `begin_edit` UX), and hand
    /// focus to the field through the focus-request mailbox.
    fn begin_edit(&self, target: EditTarget, surface: EditSurface) -> bool {
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
        // R901 — opening a *different* target commits the in-flight one first (the
        // Qt item-view discipline), so the migration never silently drops text.
        if let Some(prev) = prev {
            if prev.target != target {
                let text = self.edit_buffer.text();
                apply_edit_commit(&self.nodes, &self.undo, prev.target, &text);
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
        let nodes = self.nodes.get();
        let node = nodes.iter().find(|n| n.id == target.node())?;
        Some(match target {
            EditTarget::Title(_) => node.title.clone(),
            EditTarget::PortDefault { port, .. } => node.input_default(port)?.edit_text(),
            EditTarget::PosX(_) => node.x.to_string(),
            EditTarget::PosY(_) => node.y.to_string(),
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
        self.edges
            .get()
            .iter()
            .any(|e| e.to_node == node && e.to_port == port)
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
                } else if parse_palette_sub(s).is_some() {
                    // R850 — a palette card press: not a drag, not an input
                    // port. Recorded as its own variant so the background
                    // edge-probe is suppressed without lying about what was
                    // pressed (the activation runs on the matching PointerUp).
                    self.pending_press.set(PendingPress::Palette);
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
                // R849 — a palette card's release creates a node (the activation
                // edge); a node card's release selects it. (A palette press set
                // PendingPress::Palette above, suppressing the edge-probe; the
                // gesture is reset by `end_gesture` after this branch.)
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
                        self.apply_marquee(rect, mods);
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
                    self.begin_rename(n);
                } else if let Some((n, port)) = parse_idefault_sub(s) {
                    self.begin_edit_default(n, port);
                }
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
                self.record_moves(moves, false);
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
            .field("edges", &self.edges.get().len())
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
            // leaving the selection untouched (the Unreal / QGraphicsView
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
        // clicks / DnD) latches past the press point (Qt
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
            batch(|| follow_members(&self.nodes, &start.members, gx, gy));
        }
    }

    /// R877 §5.15 §5.49 — the canvas wheel vocabulary, riding the router's
    /// External-first offer:
    ///
    /// * `Ctrl`+wheel — zoom, anchored at the cursor (consumed).
    /// * `Shift`+wheel — horizontal pan: the vertical notches drive the
    ///   x offset, the browser/Figma convention (consumed; written
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
                let edge = self
                    .edges
                    .get()
                    .into_iter()
                    .find(|e| e.to_node == n && e.to_port == i)?;
                (edge.from_node, edge.from_port, Some(edge.id))
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

impl ExternalIntrospect for NodeGraphExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("node_count", "int"),
            ("edge_count", "int"),
            ("node_ids", "string"),
            ("edge_ids", "string"),
            // R1227 — comment-frame introspection: the annotation layer as data.
            ("frame_count", "int"),
            ("frame_ids", "string"),
            ("frame.<id>.title", "string"),
            ("frame.<id>.x", "int"),
            ("frame.<id>.y", "int"),
            ("frame.<id>.w", "int"),
            ("frame.<id>.h", "int"),
            ("frame.<id>.contains", "string"),
            ("selected", "int"),
            ("selected_ids", "string"),
            ("selected_edge", "int"),
            ("renaming", "int"),
            ("editing", "json"),
            ("begin_rename", "int"),
            ("begin_edit_default", "string"),
            ("begin_edit_detail", "string"),
            ("node.<id>.title", "string"),
            ("node.<id>.x", "int"),
            ("node.<id>.y", "int"),
            ("node.<id>.inputs", "int"),
            ("node.<id>.outputs", "int"),
            ("node.<id>.input_types", "string"),
            ("node.<id>.output_types", "string"),
            ("node.<id>.input_default.<port>", "json"),
            // R916 — the Details panel's selection-relative addressing: each
            // `detail.<field>` resolves against the *single* selected node (the
            // R909 inspector pattern, now inside the node-graph editor). `Null`
            // when the selection is not exactly one node. R917 — the mirror is
            // *complete*: every readable `node.<id>.<field>` above has a
            // `detail.<field>` twin (the delegation answers any field), so the
            // schema declares the full set, not a subset. `detail.node` is the
            // selected id, read-only (write the selection via `selected` /
            // `selected_ids`); the rest are read + intervene.
            ("detail.node", "int"),
            ("detail.title", "string"),
            ("detail.x", "int"),
            ("detail.y", "int"),
            ("detail.inputs", "int"),
            ("detail.outputs", "int"),
            ("detail.input_types", "string"),
            ("detail.output_types", "string"),
            ("detail.input_default.<port>", "json"),
            ("edge.<id>", "string"),
            ("viewport.x", "float"),
            ("viewport.y", "float"),
            ("viewport.zoom", "float"),
            ("send", "string"),
            ("add_node", "string"),
            ("frame_all", "json"),
            ("add_edge", "string"),
            ("remove_edge", "int"),
            ("reconnect_edge", "string"),
            // R1226 — the wire knife: cut every edge the segment "x1,y1,x2,y2"
            // (graph units) crosses, as one undo step. Returns the CSV of cut
            // edge ids (mirrors `edge_ids`), empty when nothing was crossed.
            ("cut_wires", "string"),
            // R1235 — splice a reroute node into edge `<int>`; returns the new
            // node id (`Null` for an unknown edge).
            ("add_reroute", "int"),
            // R1227 — comment-frame verbs: `add_frame` (no arg) frames the
            // current node selection, returning the new frame id (`Null` when
            // nothing is selected); `remove_frame` deletes a frame by id.
            ("add_frame", "int"),
            ("remove_frame", "int"),
            ("delete_node", "int"),
            ("delete_selected", "json"),
            // R1236 — dissolve = delete + reconnect through a 1-in/1-out node
            // (the reroute inverse). `dissolve_node <id>`; `dissolve_selected`
            // (no arg) dissolves the lone selected node.
            ("dissolve_node", "int"),
            ("dissolve_selected", "json"),
            ("select_all", "json"),
            ("nudge", "string"),
            // R948 — align / distribute the selection (no args; the AI-first
            // peer of an editor's align toolbar). Each returns whether the
            // graph changed.
            ("align_left", "json"),
            ("align_center_h", "json"),
            ("align_right", "json"),
            ("align_top", "json"),
            ("align_center_v", "json"),
            ("align_bottom", "json"),
            ("distribute_h", "json"),
            ("distribute_v", "json"),
            ("serialized", "string"),
            ("set_graph", "string"),
            ("save", "json"),
            ("load", "json"),
            // R1220 — the pin-drop create menu (drag off a pin → typed menu →
            // auto-wire). `pin_create` reads the open menu (Null when closed);
            // the verbs open / filter / rove / commit / cancel it — the AI-first
            // peer of the live gesture, funnelling through the same coordinator.
            ("pin_create", "json"),
            ("open_pin_create", "string"),
            ("pin_create_filter", "string"),
            ("pin_create_highlight", "string"),
            ("commit_pin_create", "string"),
            ("cancel_pin_create", "json"),
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
            // R1227 — the comment-frame enumeration handles (the annotation
            // layer as data, exactly like `node_ids` / `edge_ids`).
            "frame_count" => Some(IntrospectValue::Int(int_of(self.frames.get().len()))),
            "frame_ids" => Some(IntrospectValue::Text(csv_ids(
                self.frames.get().iter().map(|f| f.id.raw()),
            ))),
            // R879 — `selected` answers the *single*-selection question:
            // an Int only when exactly one node is selected (a multi-set
            // has no unambiguous "the" node — read `selected_ids`).
            "selected" => Some(match self.selection.get().node() {
                Some(id) => IntrospectValue::Int(i64::from(id.raw())),
                None => IntrospectValue::Null,
            }),
            // R879 — the multi-select read twin: CSV of the selected node
            // ids in id order ("" when no node selection).
            "selected_ids" => Some(IntrospectValue::Text(csv_ids(
                self.selection.get().nodes().iter().map(|id| id.raw()),
            ))),
            "selected_edge" => Some(match self.selection.get().edge() {
                Some(id) => IntrospectValue::Int(i64::from(id.raw())),
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
                }) => IntrospectValue::Int(i64::from(id.raw())),
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
            _ => {
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
                    let nodes = self.nodes.get();
                    let node = nodes.iter().find(|n| n.id == id)?;
                    return match field {
                        "title" => Some(IntrospectValue::Text(node.title.clone())),
                        "x" => Some(IntrospectValue::Int(i64::from(node.x))),
                        "y" => Some(IntrospectValue::Int(i64::from(node.y))),
                        "inputs" => Some(IntrospectValue::Int(int_of(node.inputs()))),
                        "outputs" => Some(IntrospectValue::Int(int_of(node.outputs()))),
                        // R898 — the typed-port read twins: CSV of the port
                        // types in port order ("" for a source / sink). The
                        // arity reads above stay the byte-stable count contract.
                        "input_types" => {
                            Some(IntrospectValue::Text(port_types_csv(&node.input_ports)))
                        }
                        "output_types" => {
                            Some(IntrospectValue::Text(port_types_csv(&node.output_ports)))
                        }
                        // R899 — the typed default of an input port (the
                        // write twin is `intervene node.<id>.input_default.<port>`);
                        // a `Float` reads as a float, a `Vector` (`Color`) as a
                        // `{hex,r,g,b,a}` object — the CellValue introspect shape.
                        other => other
                            .strip_prefix("input_default.")
                            .and_then(|p| p.parse::<usize>().ok())
                            .and_then(|port| {
                                node.input_default(port).map(CellValue::to_introspect)
                            }),
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
                // R1227 — `frame.<id>.<field>` reads (extracted to keep `query`
                // within the workspace line ceiling).
                self.query_frame(path)
            }
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
                apply_set_pos(&self.nodes, &self.undo, id, x, y);
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
                    if apply_rename(&self.nodes, &self.undo, id, &t) {
                        Ok(())
                    } else {
                        Err(InterveneError::OutOfRange)
                    }
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R898 — port arity and the typed-port lists are read-only: ports
            // are defined by the node kind, edited only by add/remove edges.
            "inputs" | "outputs" | "input_types" | "output_types" => Err(InterveneError::ReadOnly),
            // R899 — set an input port's typed default (the write twin of
            // `query node.<id>.input_default.<port>`); routed through the
            // type-checking [`Self::intervene_input_default`] helper.
            _ => self.intervene_input_default(id, &node, field, value),
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
            "add_edge" => match args {
                IntrospectValue::Text(s) => {
                    let (fnode, fport, tnode, tport) =
                        parse_quad(&s).ok_or(InvokeError::Rejected)?;
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
                    let (edge, tnode, tport) = parse_reconnect(&s).ok_or(InvokeError::Rejected)?;
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
            "add_frame" => Ok(self.invoke_add_frame()),
            "remove_frame" => self.invoke_remove_frame(&args),
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
            "dissolve_selected" => Ok(IntrospectValue::Bool(self.dissolve_selected())),
            // R880 — select every node (the keyboard `Ctrl`+`A` twin).
            // `false` on an empty graph.
            "select_all" => Ok(IntrospectValue::Bool(self.select_all())),
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
            "begin_edit_default" => match args {
                IntrospectValue::Text(s) => {
                    let (node, port) = parse_node_port(&s).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.begin_edit_default(node, port)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
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
                        return Some(Err(InvokeError::Rejected));
                    };
                    let Some(src) = self.node_by_id(from_node) else {
                        return Some(Ok(IntrospectValue::Bool(false)));
                    };
                    let at_graph = (src.x + NODE_W + PIN_CREATE_GAP, src.y);
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
                    Err(_) => Err(InvokeError::Rejected),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
            // Commit a create: `Text` names the kind (must be a current
            // candidate), `Null` commits the highlighted item (the Enter twin).
            // Returns the new node's id, or Rejected (menu left open).
            "commit_pin_create" => {
                let committed = match args {
                    IntrospectValue::Text(s) => {
                        match PALETTE.iter().position(|&(name, _, _)| name == s) {
                            Some(kind) => self.commit_pin_create_kind(kind),
                            None => return Some(Err(InvokeError::Rejected)),
                        }
                    }
                    IntrospectValue::Null => self.commit_pin_create_highlighted(),
                    _ => return Some(Err(InvokeError::TypeMismatch)),
                };
                committed
                    .map(|id| IntrospectValue::Int(i64::from(id.raw())))
                    .ok_or(InvokeError::Rejected)
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
    apply_edit_commit(&use_nodes(), &use_undo(), active.target, &text);
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
            port_default_kind(&use_nodes().get(), node, port).unwrap_or(CellKind::Text)
        }
        // R918 — a position is an integer: the gate accepts digits and a leading
        // sign, the same `CellKind::Int` the data-grid / property-grid use.
        EditTarget::PosX(_) | EditTarget::PosY(_) => CellKind::Int,
    }
}

/// R901 / R918 — the structured `query editing` read for an in-flight edit:
/// `{ kind, node, port?, surface }`. `kind` is `title` / `port_default` /
/// `pos_x` / `pos_y`; `surface` is `card` / `panel` (R918 — *which* surface
/// hosts the field). The honest generalisation of the R878 `query renaming` int,
/// which survives as a degenerate projection over `Title` targets.
fn active_edit_introspect(active: ActiveEdit) -> IntrospectValue {
    let mut obj = match active.target {
        EditTarget::Title(id) => serde_json::json!({ "kind": "title", "node": id.raw() }),
        EditTarget::PortDefault { node, port } => {
            serde_json::json!({ "kind": "port_default", "node": node.raw(), "port": port })
        }
        EditTarget::PosX(id) => serde_json::json!({ "kind": "pos_x", "node": id.raw() }),
        EditTarget::PosY(id) => serde_json::json!({ "kind": "pos_y", "node": id.raw() }),
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
        // R877 — frame the whole graph (Unreal / Blender `F`).
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
    let commands = vec![
        PathCommand::MoveTo(ppt(from.0, from.1)),
        PathCommand::CurveTo {
            c1: ppt(c1.0, c1.1),
            c2: ppt(c2.0, c2.1),
            end: ppt(to.0, to.1),
        },
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

/// R1230 — resolve edge `e` to its `(from output-port, to input-port)` centres,
/// dropping an edge whose endpoint node is absent. The ONE endpoint SSOT the
/// edge paint ([`view_edges`]), the click hit-test ([`NodeGraphExternal::hit_test_edge`]),
/// and the wire knife ([`NodeGraphExternal::cut_wires`]) all read — a port-anchor
/// change (offset, multi-row ports) lands once, so the three can never disagree
/// (the R1226 knife had copied `hit_test_edge`'s body verbatim as a third site).
fn edge_endpoints(nodes: &[GraphNode], e: &Edge) -> Option<((i32, i32), (i32, i32))> {
    Some((
        output_port_center(node_ref(nodes, e.from_node)?, e.from_port),
        input_port_center(node_ref(nodes, e.to_node)?, e.to_port),
    ))
}

/// All committed edges, resolved to their port centres. Painted behind the
/// node cards; the selected edge paints thicker in the highlight colour. Each
/// edge is tagged by its stable [`EdgeId`].
fn view_edges(
    nodes: &[GraphNode],
    edges: &[Edge],
    selected_edge: Option<EdgeId>,
    theme: &Theme,
    zoom: f64,
) -> Vec<Scene> {
    let color = theme.resolve(ColorRole::Accent);
    let hot = theme.resolve(ColorRole::OnSurface);
    edges
        .iter()
        .filter_map(|e| {
            let (from, to) = edge_endpoints(nodes, e)?;
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

/// R899 — the literal default value shown beside an *unconnected* input port
/// (the "pin default" label). A tagged container so an AI client can observe
/// the painted default through `scene/snapshot` (the value is also the
/// `query node.<id>.input_default.<port>` read); its `Text` child carries the
/// `CellValue::display` string. Placed just right of the port square, node-local.
fn view_input_default(tag: String, text: &str, top: i32, theme: &Theme, zoom: f64) -> Scene {
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

/// R901 — the ONE shared inline field painted over a pin's default label while
/// that port default is being edited (the [`view_input_default`] swap, the
/// pin-row twin of the header's title-or-field switch). Tagged
/// [`EDIT_TF_TAG`] by [`tf_paint::view_field`] — the field owns the hit target
/// and focus while open — and positioned where the static label sat, projected
/// through the zoom like every world coordinate.
fn view_port_default_field(edit_field: RootState, top: i32, theme: &Theme, zoom: f64) -> Scene {
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
fn view_node(
    node: &GraphNode,
    selected: bool,
    card_edit: Option<EditTarget>,
    edit_field: RootState,
    wired_inputs: &BTreeSet<usize>,
    theme: &Theme,
    zoom: f64,
) -> Scene {
    let id = node.id;
    // R878 — while this node is being renamed, the header swaps its title
    // text for the ONE shared inline rename field (the data-grid
    // title-or-field switch), sized to the header and projected through the
    // zoom like every other world coordinate.
    let head_inner = if card_edit == Some(EditTarget::Title(id)) {
        let style = tf_paint::TextFieldStyle {
            field_w: upx(wpx(NODE_W - 8, zoom)),
            field_h: upx(wpx(HEADER_H - 6, zoom)),
            field_pad: 4,
            font_size_px: wfont(NODE_TITLE_PX, zoom),
            ..tf_paint::TextFieldStyle::m3_filled()
        };
        tf_paint::view_field(
            EDIT_TF_TAG,
            edit_field.0,
            edit_field.1,
            theme,
            &style,
            "Rename node",
        )
    } else {
        Scene::Text(TextNode::styled(
            node.title.clone(),
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
    // glance — the Unreal/Blender colour-coded-pin convention.
    for (i, ty) in node.input_ports.iter().enumerate() {
        children.push(view_port(
            format!("{GRAPH_TAG}#iport_{id}_{i}"),
            0,
            port_row_top(i),
            ty.color(),
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
            if card_edit == Some(EditTarget::PortDefault { node: id, port: i }) {
                children.push(view_port_default_field(
                    edit_field,
                    port_row_top(i),
                    theme,
                    zoom,
                ));
            } else if let Some(val) = node.input_default(i) {
                children.push(view_input_default(
                    format!("{GRAPH_TAG}#idefault_{id}_{i}"),
                    &val.display(),
                    port_row_top(i),
                    theme,
                    zoom,
                ));
            }
        }
    }
    for (j, ty) in node.output_ports.iter().enumerate() {
        children.push(view_port(
            format!("{GRAPH_TAG}#oport_{id}_{j}"),
            NODE_W - PORT_SIZE,
            port_row_top(j),
            ty.color(),
            zoom,
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
                    .with_absolute_position(upx(wpx(node.x, zoom)), upx(wpx(node.y, zoom)))
                    .with_size(Size::px(
                        upx(wpx(NODE_W, zoom)),
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
/// announced — the same [`CommentFrame::contains_node`] gate the paint + the
/// `contains` query use). Extracted from `access_node` (line ceiling).
fn frame_access_nodes(frames: &[CommentFrame], nodes: &[GraphNode]) -> Vec<AccessNode> {
    frames
        .iter()
        .map(|f| {
            let members = nodes.iter().filter(|n| f.contains_node(n)).count();
            AccessNode::new(format!("{GRAPH_TAG}#frame_{}", f.id), AriaRole::Group)
                .with_name(format!("Comment frame: {} ({members} nodes)", f.title))
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
fn view_frames(frames: &[CommentFrame], zoom: f64) -> Vec<Scene> {
    let fill = Color {
        a: FRAME_FILL_ALPHA,
        ..FRAME_COLOR
    };
    frames
        .iter()
        .map(|f| {
            let title = Scene::Text(
                TextNode::styled(
                    f.title.clone(),
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
                                upx(wpx(f.w, zoom).max(1)),
                                upx(wpx(f.h, zoom).max(1)),
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
    for (idx, &(title, input_ports, output_ports)) in PALETTE.iter().enumerate() {
        let label = Scene::Text(TextNode::styled(
            format!("{title} ({}/{})", input_ports.len(), output_ports.len()),
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
fn view_pin_create_menu(pc: &PinCreate, nodes: &[GraphNode], theme: &Theme) -> Option<Scene> {
    let from_ty = node_ref(nodes, pc.from_node)?.output_type(pc.from_port)?;
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
        let &(title, input_ports, output_ports) = &PALETTE[kind];
        let label = Scene::Text(TextNode::styled(
            format!("{title} ({}/{})", input_ports.len(), output_ports.len()),
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
fn detail_rows(node: &GraphNode) -> Vec<DetailRow> {
    let id = node.id;
    let mut rows = vec![
        DetailRow {
            key: "title".to_owned(),
            label: "Title".to_owned(),
            value: node.title.clone(),
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
    for (port, ty) in node.input_ports.iter().enumerate() {
        rows.push(DetailRow {
            key: format!("in_{port}"),
            label: format!("In {port} · {}", ty.name()),
            value: node
                .input_default(port)
                .map_or_else(String::new, CellValue::display),
            target: EditTarget::PortDefault { node: id, port },
        });
    }
    rows
}

/// R916 — the Details panel: the single selected node's editable properties
/// (title / position / per-port defaults) reflected as rows, the Unreal Details
/// "select → inspect" surface. Mirrors the palette sidebar's shape (a sibling
/// column of the canvas). When the selection is not exactly one node it shows a
/// placeholder — there is no unambiguous "the" node to inspect (the `selected` /
/// `detail.node` `Null` case made visible).
fn view_details_panel(
    nodes: &[GraphNode],
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

    if let Some(node) = selection.node().and_then(|id| node_ref(nodes, id)) {
        for row in detail_rows(node) {
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

/// R899 — the node cards, one per node. Computes each node's wired input set
/// (the ports whose default label is hidden because an edge supplies their
/// value — the single source the paint and the `add_edge` open-port rule
/// share) and lowers it to a [`view_node`] card.
fn view_node_cards(
    nodes: &[GraphNode],
    edges: &[Edge],
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
    nodes
        .iter()
        .map(|node| {
            let wired_inputs: BTreeSet<usize> = edges
                .iter()
                .filter(|e| e.to_node == node.id)
                .map(|e| e.to_port)
                .collect();
            // R901 — only the card hosting the in-flight edit paints the shared
            // field (a title or one pin default); every other card paints
            // statically. `None` once the target's node is a different card.
            let card_edit = card_edit_target.filter(|t| t.node() == node.id);
            view_node(
                node,
                selected.contains(&node.id),
                card_edit,
                edit_field,
                &wired_inputs,
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
    let nodes = use_nodes().get();
    let edges = use_edges().get();
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
    world_children.extend(view_frames(&use_frames().get(), zoom));

    // Edges (behind) → preview wire → node cards (on top).
    world_children.extend(view_edges(&nodes, &edges, selected_edge, &theme, zoom));

    if let Some(p) = preview {
        if let Some(from_node) = node_ref(&nodes, p.from_node) {
            let from = output_port_center(from_node, p.from_port);
            let to =
                p.to.and_then(|(tn, tp)| Some(input_port_center(node_ref(&nodes, tn)?, tp)))
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
        &nodes, &edges, &selected, active, state, &theme, zoom,
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
            node_ref(&nodes, *id).map_or("—", |n| n.title.as_str())
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
        nodes.len(),
        edges.len(),
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
        if let Some(menu) = view_pin_create_menu(&pc, &nodes, &theme) {
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
            view_details_panel(&nodes, &selection, active, state, &theme),
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
            use_nodes(),
            use_edges(),
            use_selection(),
            use_preview(),
            use_next_edge_id(),
            use_next_node_id(),
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
                frames: use_frames(),
                next_frame_id: use_next_frame_id(),
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
    /// [`TextFieldExternal`] modal member (the R790 todomvc `EDIT_TF`
    /// shape): one field reused for every node title AND every input port
    /// default, painted only while an edit is in flight, raising the R793
    /// commit-on-blur intent on a click-away.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let editor_state = use_text_edit_state(EDIT_TF_TAG);
        let blink = use_caret_blink(EDIT_TF_TAG);
        vec![
            ExtraExternal::new(UNDO_TAG, Box::new(UndoStackExternal::new(use_undo()))),
            ExtraExternal::new(
                EDIT_TF_TAG,
                Box::new(
                    TextFieldExternal::new()
                        .attach_state(editor_state)
                        .attach_blink(blink)
                        .with_blur_intent(),
                ),
            ),
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
    nodes: &[GraphNode],
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
    if let Some(node) = selection.node().and_then(|id| node_ref(nodes, id)) {
        for row in detail_rows(node) {
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
        let nodes = use_nodes().get();
        let frames = use_frames().get();
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
        let palette_names: Vec<String> = PALETTE
            .iter()
            .map(|&(t, _, _)| format!("Add {t}"))
            .collect();
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
            let from_ty = node_ref(&nodes, pc.from_node)?.output_type(pc.from_port)?;
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
        for node in &nodes {
            group = group.with_child(format!("{GRAPH_TAG}#node_{}", node.id));
        }
        // R1227 — the comment frames are also children of the canvas group (a
        // labelled `group` per frame, reachable in the AT tree, not an orphan).
        for f in &frames {
            group = group.with_child(format!("{GRAPH_TAG}#frame_{}", f.id));
        }
        // R1220 — link the open create menu into the canvas group so its
        // `menu` subtree is reachable (not an orphan in the AT tree).
        if pin_menu.is_some() {
            group = group.with_child(format!("{GRAPH_TAG}#{PIN_MENU_SUB}"));
        }
        out.push(group);
        out.extend(frame_access_nodes(&frames, &nodes));
        for node in &nodes {
            let mut entry =
                AccessNode::new(format!("{GRAPH_TAG}#node_{}", node.id), AriaRole::Generic)
                    .with_name(format!(
                        "{} ({} in, {} out)",
                        node.title,
                        node.inputs(),
                        node.outputs()
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
            &nodes, &selection, active, state.0, focused,
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
