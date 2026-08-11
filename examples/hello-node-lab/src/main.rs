// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-node-lab` — R1651 §5.21 §5.51 §2 #7 — the analysis-tool **node graph
//! lab**, assembled as one application against a written-down specification of
//! the reference screen.
//!
//! ## What is new here, and why it is a crate and not this example
//!
//! The analysis-tool census puts a *node inspector that is the settings editor*
//! in its must-have tier and, measured at R1646, scored it `gap`: no crate held
//! a property grid, so the per-field applies badge the capability names had
//! nowhere to live. R1650 gave the **model** a home; R1651 gives it a painter
//! (`pinion_widget_paint::config_form`) and this screen is its first consumer.
//! What the inspector on the right does — a row per configuration path, a
//! HOT/RESTART badge per row, a defect shown on the row it is about, a launch
//! verdict derived from the rows rather than set beside them, and a deployable
//! document derived from the same rows — is framework code, not this file's.
//!
//! ## The specification is a value
//!
//! [`spec`] holds the reference screen as a table: which panes exist and how
//! wide, which roles the palette groups, which nodes the opening graph holds
//! and where, which fields the inspector shows and with what applies-scope.
//! This file is written *against* that table, the table is published on the
//! wire as `spec`, and the demo asserts the painted scene against it in **both
//! directions** — an element the screen is missing and an element the screen
//! invented are both failures. A round can claim it reproduced a reference; the
//! only thing that makes the claim checkable is putting the reference where a
//! machine can read it.
//!
//! ## The graph is the crate's, the taxonomy is this application's
//!
//! Nodes, frames, links and reachability are `pinion_node_graph::Document`, so
//! a link the model cannot hold is refused by the crate and named. What roles
//! exist, and what their pins carry, is [`graph::Role`] / [`graph::Transport`]
//! — which is exactly the split the census draws when it calls the palette
//! `app` and `connect` `have`.
//!
//! ## Run it
//!
//! ```text
//! cargo run -p hello-node-lab --release
//! ```
//!
//! Drag empty canvas to pan, wheel to zoom, drag a node to place it (hold ctrl
//! to snap), drag a pin to author a link, click a node to inspect it, click a
//! link to see its endpoint. The gate refuses to open Run while a value would
//! fail at start-up, and says what stands when it does open.
//!
//! See `tools/demos/r1651_the_node_lab_matches_the_reference.py`.

mod graph;
mod spec;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Signal, Tracked};
use pinion_core::scene::{
    ContainerNode, PathCommand, PathNode, PathPoint, Rect, ScrollAxis, ScrollNode, TextNode,
};
use pinion_core::style::{
    Border, BoxStyle, Color, LayoutStyle, PathStyle, Size, Stroke, TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::config_form::{
    Applies, ConfigDefect, ConfigField, ConfigForm, FieldType, Verdict,
};
use pinion_core::widgets::scroll::AutoScroll;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_node_graph::{Document, Item, LinkId, NodeBody, NodeId, ROOT, Side, Socket};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::config_form::{
    FieldGrowth, FormGeometry, FormStyle, RowWrap, form_geometry, row_access_nodes,
    view_config_form,
};

use graph::{LabNode, Role, Transport};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloNodeLabRenderer, HelloNodeLabRendererError);

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = 1440;
const WIN_H: u32 = 900;
const VIEW_TAG: &str = "node_lab";
const THEME_TAG: &str = "app";

const RAIL_W: u32 = spec::PANES[0].width;
const PALETTE_W: u32 = spec::PANES[1].width;
const INSP_W: u32 = spec::PANES[3].width;
const APP_BAR_H: u32 = spec::APP_BAR_H;
const TOOLBAR_H: u32 = spec::TOOLBAR_H;
const PAD: u32 = 14;

const FONT_TITLE: u32 = 14;
const FONT_BODY: u32 = 12;
const FONT_SMALL: u32 = 11;
const FONT_TINY: u32 = 9;

/// The canvas: what is left between the palette and the inspector.
/// The window this frame is being painted into.
///
/// ★ R1654 — read from the shell rather than assumed. The screen used to be
/// [`WIN_W`] x [`WIN_H`] constants everywhere, so enlarging the window left the
/// content in the top-left corner with the rest of the surface black, and
/// shrinking it painted the inspector off the edge. Reported from the running
/// window; invisible to every test here, because a test that calls
/// `compute_layout(scene, WIN_W, WIN_H)` has assumed the very thing that was
/// wrong.
///
/// `use_viewport_size` is a tracked read, so the view re-runs on a resize. It
/// answers `(0, 0)` where no shell has published one — a headless probe, a unit
/// test — and the declared design size is the honest fallback there: it is what
/// the specification's rectangles were measured against.
fn window_size() -> (u32, u32) {
    // The hook is strict about the owner scope by design. Off a scope entirely
    // (a bare unit call) the design size is the answer, and asking politely is
    // how this stays callable from both.
    let live =
        pinion_core::reactive::Owner::current().map(|_| pinion_core::reactive::use_viewport_size());
    match live {
        Some((w, h)) if w >= MIN_W && h >= MIN_H => (w, h),
        _ => (WIN_W, WIN_H),
    }
}

/// The smallest window this screen lays out in.
///
/// Below it the panes would overlap, so the layout stops shrinking and the
/// window clips instead — the same choice a fixed minimum size makes, stated
/// here rather than left to arithmetic that would produce negative widths.
const MIN_W: u32 = RAIL_W + PALETTE_W + 240 + INSP_W;
/// The smallest height, likewise.
const MIN_H: u32 = APP_BAR_H + TOOLBAR_H + 200;

fn canvas_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(
        RAIL_W + PALETTE_W,
        APP_BAR_H + TOOLBAR_H,
        w - RAIL_W - PALETTE_W - INSP_W,
        h - APP_BAR_H - TOOLBAR_H,
    )
}

fn palette_rect() -> Rect {
    Rect::new(RAIL_W, APP_BAR_H, PALETTE_W, window_size().1 - APP_BAR_H)
}

fn inspector_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(w - INSP_W, APP_BAR_H, INSP_W, h - APP_BAR_H)
}

fn rail_rect() -> Rect {
    Rect::new(0, APP_BAR_H, RAIL_W, window_size().1 - APP_BAR_H)
}

fn toolbar_rect() -> Rect {
    let (w, _) = window_size();
    Rect::new(
        RAIL_W + PALETTE_W,
        APP_BAR_H,
        w - RAIL_W - PALETTE_W - INSP_W,
        TOOLBAR_H,
    )
}

/// The height of one palette row, and the gap under a group heading.
const PAL_ROW_H: u32 = 40;
const PAL_HEAD_H: u32 = 22;
/// A node card's header band.
const CARD_HDR: u32 = 26;
/// One key/value line inside a node card.
const CARD_ROW_H: u32 = 15;
/// A pin's diameter.
const PIN: u32 = 11;
/// The zoom range, in percent, and the step a press moves it.
const ZOOM_MIN: u32 = 25;
const ZOOM_MAX: u32 = 400;
const ZOOM_STEP: u32 = 8;
/// The grid a ctrl-held node drag snaps to.
const SNAP: i32 = 22;

/// (R1653) Where world unit `0` sits inside the world surface.
///
/// The surface is a fixed extent the canvas viewport slides over, so a
/// coordinate on it is unsigned; the margin is what lets a node dragged to a
/// negative world position, or a pan to the left, still land on it.
const WORLD_ORIGIN: i32 = 2_000;
/// The world surface's extent, both axes.
const WORLD: i32 = WORLD_ORIGIN * 2 + 2_400;

// ── The reference's own colour tokens ───────────────────────────────────────

const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// The lab's palette.
///
/// Two of these resolve from the theme's own roles rather than from a literal —
/// `Warning` and `Error` — because the gate's two outcomes are a framework
/// vocabulary (`ConfigDefect::blocks`) and painting them from a local constant
/// would be a second opinion about what "this warns" looks like. R1651 added
/// the warning tier to the theme for exactly this row.
#[derive(Clone, Copy)]
struct Ink {
    bg: Color,
    surface: Color,
    raised: Color,
    outline: Color,
    outline_2: Color,
    text: Color,
    text_2: Color,
    text_3: Color,
    accent: Color,
    accent_soft: Color,
    accent_line: Color,
    ok: Color,
    warn: Color,
    err: Color,
    grid: Color,
}

fn ink(theme: &Theme) -> Ink {
    Ink {
        bg: rgb(0x0E_0F12),
        surface: rgb(0x16_181D),
        raised: rgb(0x1E_2127),
        outline: rgb(0x2A_2E36),
        outline_2: rgb(0x3A_404B),
        text: rgb(0xE8_EBEF),
        text_2: rgb(0x98_A2AD),
        text_3: rgb(0x69_7180),
        accent: rgb(0xEC_5AA0),
        accent_soft: Color::rgba(0x9A, 0x00, 0x4F, 0x60),
        accent_line: Color::rgba(0xEC, 0x5A, 0xA0, 0x73),
        ok: rgb(0x35_C08B),
        warn: theme.resolve(ColorRole::Warning),
        err: theme.resolve(ColorRole::Error),
        grid: rgb(0x20_242C),
    }
}

/// The colour a transport is drawn in, which is what an accept pin's ring
/// means.
const fn transport_ink(transport: Transport) -> Color {
    match transport {
        Transport::Tcp => rgb(0x2D_6CDF),
        Transport::Tls => rgb(0x1F_8A4C),
        Transport::Quic => rgb(0x7C_4DEF),
        Transport::Udp => rgb(0xC7_7800),
        Transport::Ws => rgb(0x3E_7C8C),
    }
}

/// The colour a role is drawn in on the canvas card and the palette swatch.
const fn role_ink(role: Role) -> Color {
    match role {
        Role::Router => rgb(0xEC_5AA0),
        Role::Peer => rgb(0x2D_6CDF),
        Role::Client => rgb(0x69_7180),
        Role::Store => rgb(0x1F_8A4C),
        Role::Publisher | Role::Subscriber => rgb(0x8A_5CF6),
        Role::Querier | Role::Responder => rgb(0xC7_7800),
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// A drag in flight, and which kind it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum Drag {
    /// The canvas is being panned from this cursor position.
    Pan { from: (u32, u32), start: (i32, i32) },
    /// A node is being placed. Held: the node, the grab offset in canvas units.
    Node {
        node: NodeId,
        grab: (i32, i32),
        snap: bool,
    },
    /// A link is being authored out of this node's dial pin.
    Wire { from: NodeId },
    /// A host frame is being moved, and every card it holds moves with it.
    Frame { frame: NodeId, from: (i32, i32) },
}

/// Everything the screen is.
struct LabState {
    doc: Tracked<Document<LabNode>>,
    /// Which node each identifier is, so the wire can address a node the way
    /// the screen labels it rather than by an internal number.
    ids: RefCell<BTreeMap<String, NodeId>>,
    forms: Tracked<BTreeMap<NodeId, ConfigForm>>,
    frames: RefCell<BTreeMap<NodeId, String>>,
    selected: Signal<Option<NodeId>>,
    selected_link: Signal<Option<LinkId>>,
    zoom: Signal<u32>,
    pan: Signal<(i32, i32)>,
    running: Signal<bool>,
    /// The master auto-discovery switch: off by default, because a graph whose
    /// links are all authored is the one whose behaviour is a function of what
    /// is on the canvas.
    discovery: Signal<bool>,
    cursor: Signal<(u32, u32)>,
    drag: Signal<Option<Drag>>,
    pressed: RefCell<Option<Hit>>,
    toast: Signal<String>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<LabState>>> = const { RefCell::new(None) };
}

fn use_lab_state() -> Rc<LabState> {
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(state) = slot.as_ref() {
            return Rc::clone(state);
        }
        let state = Rc::new(LabState::opening());
        *slot = Some(Rc::clone(&state));
        state
    })
}

impl LabState {
    /// The graph the screen opens with — built from [`spec`], so the screen and
    /// the specification cannot disagree about what it holds.
    fn opening() -> Self {
        let mut doc: Document<LabNode> = Document::new(spec::GRAPH_NAME);
        let mut ids = BTreeMap::new();
        let mut frames = BTreeMap::new();
        let mut frame_ids: BTreeMap<&str, NodeId> = BTreeMap::new();

        for frame in spec::FRAMES {
            let (x, y, _, _) = frame.rect;
            let id = doc
                .add_node(
                    ROOT,
                    NodeBody::Frame,
                    i32::try_from(x).unwrap_or(0),
                    i32::try_from(y).unwrap_or(0),
                )
                .expect("the root tree exists");
            if let Some(node) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(id)) {
                node.label = Some(format!("{} · {}", frame.name, frame.gist));
            }
            frame_ids.insert(frame.name, id);
            frames.insert(id, frame.name.to_owned());
        }

        let mut forms = BTreeMap::new();
        seed_nodes(&mut doc, &frame_ids, &mut ids, &mut forms);

        let mut selected_link = None;

        for (from, to) in spec::LINKS {
            let (Some(&a), Some(&b)) = (ids.get(*from), ids.get(*to)) else {
                continue;
            };
            // Port 0 on each side: the taxonomy declares exactly one dial and
            // at most one accept, so the index is not a guess.
            // Land on the first accept port nothing has taken, growing the run
            // when they are all busy: a router is dialled by four peers on one
            // pin, and the run is how the model holds that.
            let taken: Vec<u32> = doc
                .tree(ROOT)
                .map(|t| {
                    t.links()
                        .iter()
                        .filter(|l| l.to.node == b)
                        .map(|l| l.to.port)
                        .collect()
                })
                .unwrap_or_default();
            let arity = doc
                .signature(ROOT, b)
                .map_or(0, |s| u32::try_from(s.inputs.len()).unwrap_or(0));
            let port = (0..arity).find(|p| !taken.contains(p)).unwrap_or_else(|| {
                doc.insert_item(ROOT, b, Side::Input, arity, Item::plain())
                    .ok();
                arity
            });
            if let Ok(made) = doc.connect(ROOT, Socket::new(a, 0), Socket::new(b, port)) {
                if (*from, *to) == spec::SELECTED_LINK {
                    selected_link = Some(made.link);
                }
            }
        }

        let selected = ids.get(spec::SELECTED_NODE).copied();
        Self {
            doc: Tracked::new(doc),
            ids: RefCell::new(ids),
            forms: Tracked::new(forms),
            frames: RefCell::new(frames),
            selected: Signal::new(selected),
            selected_link: Signal::new(selected_link),
            zoom: Signal::new(spec::OPENING_ZOOM),
            pan: Signal::new((0, 0)),
            running: Signal::new(false),
            discovery: Signal::new(false),
            cursor: Signal::new((0, 0)),
            drag: Signal::new(None),
            pressed: RefCell::new(None),
            toast: Signal::new(String::new()),
        }
    }

    fn say(&self, what: impl Into<String>) {
        self.toast.set(what.into());
    }

    fn node_of(&self, id: &str) -> Option<NodeId> {
        self.ids.borrow().get(id).copied()
    }

    fn name_of(&self, node: NodeId) -> String {
        self.ids
            .borrow()
            .iter()
            .find(|(_, v)| **v == node)
            .map_or_else(|| format!("#{}", node.0), |(k, _)| k.clone())
    }

    fn role_of(&self, node: NodeId) -> Option<Role> {
        match self.doc.borrow().tree(ROOT)?.node(node)?.body {
            NodeBody::Kind(ref kind) => Some(kind.role),
            _ => None,
        }
    }

    /// Every node the canvas draws a card for: the declared ones in
    /// specification order, then anything added since.
    ///
    /// ★ Derived from the **document**, not from the specification table. It
    /// was the table until the real-pointer run showed a node added from the
    /// palette never reaching the canvas — the specification says what the
    /// screen OPENS with, and a screen that could only ever draw that is a
    /// picture.
    fn cards(&self) -> Vec<NodeId> {
        let opening: Vec<NodeId> = spec::NODES
            .iter()
            .filter_map(|n| self.node_of(n.id))
            .collect();
        let doc = self.doc.borrow();
        let Some(tree) = doc.tree(ROOT) else {
            return opening;
        };
        let mut all = opening.clone();
        for node in tree.nodes() {
            if matches!(node.body, NodeBody::Kind(_)) && !opening.contains(&node.id) {
                all.push(node.id);
            }
        }
        all
    }

    /// The gate's defects: every form's own, plus the two the *graph* raises.
    ///
    /// Both graph warnings are derived rather than listed. A node whose role can
    /// be dialled and whose listen endpoint is empty is a pin nobody can reach;
    /// a node that has turned discovery on can acquire links this canvas did not
    /// author, which is the same fact the master switch states for the graph.
    fn defects(&self) -> Vec<(String, ConfigDefect)> {
        let mut found = Vec::new();
        for node in self.cards() {
            let name = self.name_of(node);
            if let Some(form) = self.forms.borrow().get(&node) {
                for defect in form.defects() {
                    found.push((name.clone(), defect));
                }
                let listens = form
                    .field("listen.endpoints")
                    .is_some_and(|f| !f.value().trim().is_empty());
                if self.role_of(node).is_some_and(Role::accepts) && !listens {
                    found.push((
                        name.clone(),
                        ConfigDefect::UnknownKey {
                            key: format!("{name} · listen.endpoints"),
                        },
                    ));
                }
                if form
                    .field("discovery.multicast")
                    .is_some_and(|f| f.value().trim() == "true")
                {
                    found.push((
                        name.clone(),
                        ConfigDefect::UnknownKey {
                            key: format!("{name} · discovery.multicast"),
                        },
                    ));
                }
            }
        }
        found
    }

    fn verdict(&self) -> Verdict {
        let defects: Vec<ConfigDefect> = self.defects().into_iter().map(|(_, d)| d).collect();
        Verdict::over(&defects)
    }

    /// The sentence the gate shows for a defect — the framework's when the
    /// framework raised it, this application's when it did.
    fn gate_lines(&self) -> Vec<(bool, String)> {
        let mut lines = Vec::new();
        for (who, defect) in self.defects() {
            let sentence = match &defect {
                ConfigDefect::UnknownKey { key } if key.ends_with("listen.endpoints") => {
                    format!("{who} · nothing is listening, so no node can dial it")
                }
                ConfigDefect::UnknownKey { key } if key.ends_with("discovery.multicast") => {
                    format!("{who} · discovery is on, so links may appear that nobody authored")
                }
                other => format!("{who} · {}", other.sentence()),
            };
            lines.push((defect.blocks(), sentence));
        }
        lines
    }

    fn link_count(&self) -> usize {
        self.doc.borrow().tree(ROOT).map_or(0, |t| t.links().len())
    }

    /// How many links arrive at, and leave, a node.
    fn degree(&self, node: NodeId) -> (usize, usize) {
        let doc = self.doc.borrow();
        let Some(tree) = doc.tree(ROOT) else {
            return (0, 0);
        };
        let inbound = tree.links().iter().filter(|l| l.to.node == node).count();
        let outbound = tree.links().iter().filter(|l| l.from.node == node).count();
        (inbound, outbound)
    }
}

/// Put every declared node on the canvas, in its declared frame, holding the
/// form its role opens with.
///
/// A node's transport — which is the colour its accept pin is drawn in and the
/// type a link to it must match — is **derived from its endpoint** rather than
/// declared beside it, so the canvas cannot show a colour the configuration
/// does not have.
fn seed_nodes(
    doc: &mut Document<LabNode>,
    frame_ids: &BTreeMap<&str, NodeId>,
    ids: &mut BTreeMap<String, NodeId>,
    forms: &mut BTreeMap<NodeId, ConfigForm>,
) {
    for node in spec::NODES {
        let role = Role::from_name(node.role).expect("the spec names a role that exists");
        let form = form_for(node.id, role);

        let listen = form
            .field("listen.endpoints")
            .map(|f| f.value().to_owned())
            .unwrap_or_default();
        let transport = Transport::of_locator(&listen)
            .or_else(|| {
                form.field("connect.endpoints")
                    .and_then(|f| Transport::of_locator(f.value()))
            })
            .unwrap_or(Transport::Tcp);
        let (x, y, _) = node.rect;
        let id = doc
            .add_node(
                ROOT,
                NodeBody::Kind(LabNode {
                    role,
                    transport,
                    listening: !listen.is_empty(),
                }),
                i32::try_from(x).unwrap_or(0),
                i32::try_from(y).unwrap_or(0),
            )
            .expect("the root tree exists");
        if let Some(slot) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(id)) {
            slot.label = Some(node.id.to_owned());
        }
        if let Some(&frame) = frame_ids.get(node.frame) {
            doc.set_parent(ROOT, id, Some(frame)).ok();
        }
        ids.insert(node.id.to_owned(), id);
        forms.insert(id, form);
    }
}

/// The configuration form a node of that role opens with.
///
/// The five rows the reference shows on its selected node are the specification
/// for the router; every other role gets the same shape with its own opening
/// values, because a form whose rows depended on which node was clicked would
/// make "the key is the configuration path" untrue for all but one of them.
fn form_for(id: &str, role: Role) -> ConfigForm {
    let listen = match id {
        "R-01" => "tcp/0.0.0.0:7447",
        "P-01" => "tcp/0.0.0.0:7448",
        "P-02" => "tcp/0.0.0.0:7449",
        "P-03" => "tcp/0.0.0.0:7451",
        _ if role.accepts() => "",
        _ => "",
    };
    let mut fields = vec![
        ConfigField::new("id", "text", Applies::Restart, opening_id(id)),
        ConfigField::new("listen.endpoints", "locator[]", Applies::Restart, listen).with_shape(
            FieldType::List {
                of: Box::new(FieldType::Text),
            },
        ),
        ConfigField::new(
            "connect.endpoints",
            "locator[]",
            Applies::Hot,
            opening_connect(id),
        )
        .with_shape(FieldType::List {
            of: Box::new(FieldType::Text),
        }),
        ConfigField::new(
            "control.permissions",
            "perm",
            Applies::Restart,
            if role == Role::Router {
                "read, write"
            } else {
                "read"
            },
        )
        .with_shape(FieldType::Flags {
            of: vec!["read".into(), "write".into()],
        }),
        ConfigField::new(
            "transport.link.tx.batch_size",
            "int",
            Applies::Restart,
            "65535",
        )
        .with_shape(FieldType::Integer { min: 0, max: 65535 }),
    ];
    // The two peers the reference draws with a warning dot have discovery on.
    if matches!(id, "P-01" | "P-02") {
        fields.push(
            ConfigField::new("discovery.multicast", "bool", Applies::Restart, "true")
                .with_shape(FieldType::Boolean),
        );
    }
    let addable = spec::ADDABLE
        .iter()
        .filter(|key| !fields.iter().any(|f| f.key() == **key))
        .map(|key| offered(key))
        .collect();
    ConfigForm::new(fields, addable)
}

fn opening_id(id: &str) -> &'static str {
    match id {
        "R-01" => "a1",
        "P-01" => "b1",
        "P-02" => "b2",
        "P-03" => "b3",
        "S-01" => "c1",
        "T-01" => "t1",
        "T-02" => "t2",
        _ => "q1",
    }
}

fn opening_connect(id: &str) -> &'static str {
    match id {
        "R-01" => "tcp/10.0.0.21:7449",
        "T-01" | "Q-01" => "tcp/10.0.0.11:7448",
        _ => "tcp/10.0.0.10:7447",
    }
}

/// A key the inspector offers to add, with the shape it will hold.
fn offered(key: &str) -> ConfigField {
    match key {
        "discovery.multicast" | "timestamping" | "compression" => {
            ConfigField::new(key.to_owned(), "bool", Applies::Restart, "false")
                .with_shape(FieldType::Boolean)
        }
        "qos.priority" => ConfigField::new(key.to_owned(), "int", Applies::Hot, "5")
            .with_shape(FieldType::Integer { min: 0, max: 7 }),
        "routing.mode" => {
            ConfigField::new(key.to_owned(), "mode", Applies::Restart, "peer_to_peer").with_shape(
                FieldType::Choice {
                    of: vec!["peer_to_peer".into(), "client".into(), "router".into()],
                },
            )
        }
        _ => ConfigField::new(key.to_owned(), "name[]", Applies::Restart, "").with_shape(
            FieldType::List {
                of: Box::new(FieldType::Text),
            },
        ),
    }
}

// ── Canvas transform ────────────────────────────────────────────────────────

/// Canvas units to window pixels, under the current zoom and pan.
fn to_content(state: &LabState, cx: i32, cy: i32) -> (u32, u32) {
    let zoom = f64::from(state.zoom.get()) / 100.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a canvas point times a zoom is a pixel inside the world surface"
    )]
    let scale = |v: i32| (f64::from(v) * zoom) as i32;
    let origin = WORLD_ORIGIN;
    // The margin is in WORLD units and scales with the surface, so the range a
    // position may take does not shrink when the zoom grows. `clamp_to_world`
    // is what keeps this conversion total; the saturation below is the
    // belt-and-braces half of the same statement.
    (
        u32::try_from(scale(origin + cx)).unwrap_or(0),
        u32::try_from(scale(origin + cy)).unwrap_or(0),
    )
}

/// The range a node's world position may take.
///
/// ★ A bound stated where positions are SET, rather than a saturation where
/// they are painted. The world surface is finite, so some answer has to be
/// given for a position outside it, and a silent clamp at paint time is the bad
/// one: the node keeps a coordinate nothing can draw and the card appears in
/// the corner with no explanation. Clamping the drag says the same thing where
/// the user can see it — the node stops at the edge of the world.
const fn clamp_to_world(v: i32) -> i32 {
    if v < -WORLD_ORIGIN {
        -WORLD_ORIGIN
    } else if v > WORLD - WORLD_ORIGIN {
        WORLD - WORLD_ORIGIN
    } else {
        v
    }
}

/// Where the world surface is held against the viewport, which is what the pan
/// gesture moves.
///
/// ★ R1653 — the pan used to be added to every world rectangle and the result
/// converted back to pane-local by subtracting the pane's origin in `u32`. A
/// pan to the LEFT makes that subtraction underflow: the debug build panics and
/// the release build wraps to a coordinate near four billion, so a screen whose
/// hint strip advertises "drag empty space = pan" crashed on half of the
/// gesture it advertised. Nothing saw it because every gate drove the screen
/// from its opening state, where the pan is zero.
///
/// The fix is not a clamp. A pan is a *viewport* moving over a surface, the
/// framework has that primitive, and using it also gives the pane the clipping
/// it never had — panned content used to be painted over the palette and the
/// inspector rather than cut off at the canvas edge.
fn world_offset(state: &LabState, pan: (i32, i32)) -> (i32, i32) {
    let origin = i32::try_from(scaled(state, WORLD_ORIGIN.unsigned_abs())).unwrap_or(i32::MAX);
    (origin - pan.0, origin - pan.1)
}

/// A window point in the coordinates the world surface is painted in.
fn window_to_content(state: &LabState, px: u32, py: u32) -> (i64, i64) {
    let canvas = canvas_rect();
    let (ox, oy) = world_offset(state, state.pan.get());
    (
        i64::from(px) - i64::from(canvas.x) + i64::from(ox),
        i64::from(py) - i64::from(canvas.y) + i64::from(oy),
    )
}

/// The inverse of [`window_to_content`]: where a point on the world surface
/// lands in the window, or `None` when the viewport is not over it.
///
/// `None` is the honest answer rather than a clamped coordinate: the canvas
/// clips, so a point the viewport has scrolled past is not on screen at all,
/// and handing back the nearest visible pixel would let a caller press
/// something the user cannot see.
///
/// Only the tests need this direction — the screen paints in surface
/// coordinates and resolves presses into them, so nothing in the running app
/// ever converts back. It is `cfg(test)` rather than dead production code, and
/// `r1653_the_two_canvas_conversions_invert_each_other` is what keeps it from
/// drifting away from the forward one it is supposed to invert.
#[cfg(test)]
fn content_to_window(state: &LabState, cx: i64, cy: i64) -> Option<(u32, u32)> {
    let canvas = canvas_rect();
    let (ox, oy) = world_offset(state, state.pan.get());
    let x = cx - i64::from(ox) + i64::from(canvas.x);
    let y = cy - i64::from(oy) + i64::from(canvas.y);
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    contains(canvas, x, y).then_some((x, y))
}

/// Does this world-surface rectangle hold that content-space point?
const fn holds(rect: Rect, cx: i64, cy: i64) -> bool {
    cx >= rect.x as i64
        && cx < (rect.x + rect.w) as i64
        && cy >= rect.y as i64
        && cy < (rect.y + rect.h) as i64
}

/// Window pixels back to canvas units — the exact inverse of [`to_content`]
/// composed with [`world_offset`].
///
/// Lifted at its third copy: the node drag, the press that starts one and the
/// palette's "put it in the middle" each need it, and three sites free to
/// disagree about a transform's inverse is how a drag ends up half a pan out.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a window pixel divided by a zoom is a canvas unit; both fit an i32"
)]
fn to_canvas(state: &LabState, px: u32, py: u32) -> (i32, i32) {
    let canvas = canvas_rect();
    let (pan_x, pan_y) = state.pan.get();
    let zoom = f64::from(state.zoom.get()) / 100.0;
    let unscale = |v: i64| (v as f64 / zoom) as i32;
    (
        unscale(i64::from(px) - i64::from(canvas.x) - i64::from(pan_x)),
        unscale(i64::from(py) - i64::from(canvas.y) - i64::from(pan_y)),
    )
}

fn scaled(state: &LabState, v: u32) -> u32 {
    v * state.zoom.get() / 100
}

/// The digest lines a node's card shows.
///
/// The declared ones for a node the specification opens with, and otherwise the
/// first three rows of the node's own form — so a node added from the palette
/// is a card like any other rather than an empty box.
fn card_rows(state: &LabState, node: NodeId) -> Vec<(String, String)> {
    let name = state.name_of(node);
    if let Some(declared) = spec::NODES.iter().find(|n| n.id == name) {
        // ★ R1651.1 — the KEYS are the specification's (they are a per-role
        // digest, and which fields are worth showing is a design decision), but
        // the VALUES are re-read from the form whenever the form has that path.
        // They were the table's until an audit edited an endpoint and watched
        // the card keep the old one: a card showing a frozen copy of the
        // configuration is a second source, and the whole round argues against
        // exactly that.
        let forms = state.forms.borrow();
        let form = forms.get(&node);
        return declared
            .rows
            .iter()
            .map(|(k, v)| {
                let live = form
                    .and_then(|f| digest_path(k).and_then(|path| f.field(path)))
                    .map(|f| f.value().to_owned());
                ((*k).to_owned(), live.unwrap_or_else(|| (*v).to_owned()))
            })
            .collect();
    }
    state
        .forms
        .borrow()
        .get(&node)
        .map_or_else(Vec::new, |form| {
            form.fields()
                .iter()
                .take(3)
                .map(|f| (f.key().to_owned(), f.value().to_owned()))
                .collect()
        })
}

/// The configuration path a card's digest line is about, when it is about one.
///
/// A card row is a *label* a person reads at a glance (`listen`), and the form
/// is keyed by the configuration path (`listen.endpoints`); this is the one
/// mapping between them, so a digest line that names a path tracks it.
const fn digest_path(key: &str) -> Option<&'static str> {
    match key.as_bytes() {
        b"listen" => Some("listen.endpoints"),
        b"id" => Some("id"),
        b"control" => Some("control.permissions"),
        b"discovery" => Some("discovery.multicast"),
        _ => None,
    }
}

/// The width a node's card is drawn at, in canvas units.
fn card_width(state: &LabState, node: NodeId) -> u32 {
    let name = state.name_of(node);
    spec::NODES
        .iter()
        .find(|n| n.id == name)
        .map_or(146, |declared| declared.rect.2)
}

/// The rectangle a node's card occupies, in window pixels.
fn card_rect(state: &LabState, node: NodeId) -> Option<Rect> {
    let (nx, ny) = {
        let doc = state.doc.borrow();
        let held = doc.tree(ROOT)?.node(node)?;
        (held.x, held.y)
    };
    let (x, y) = to_content(state, nx, ny);
    let rows = u32::try_from(card_rows(state, node).len()).unwrap_or(0);
    Some(Rect::new(
        x,
        y,
        scaled(state, card_width(state, node)),
        scaled(state, CARD_HDR + rows * CARD_ROW_H + 6),
    ))
}

/// A pin's rectangle. `dial` is the outgoing pin on the right edge.
///
/// ★ R1653 — the pin SCALES with the canvas, because it is part of the diagram
/// rather than chrome over it. Held at a fixed size it kept its pixels while
/// the cards shrank, so at the minimum zoom a dial pin and its neighbour's
/// accept pin covered the same pixels and the one drawn second could not be
/// pressed at all — the painted screen offered a control the pointer could
/// never reach, which is the class this round exists to make visible.
fn pin_rect(state: &LabState, card: Rect, dial: bool) -> Rect {
    let pin = scaled(state, PIN).max(3);
    let y = card.y + scaled(state, CARD_HDR).max(4) / 2;
    if dial {
        Rect::new(card.x + card.w.saturating_sub(pin / 2), y, pin, pin)
    } else {
        Rect::new(card.x.saturating_sub(pin / 2), y, pin, pin)
    }
}

/// The frames the document holds, in declaration order.
fn frames_of(state: &LabState) -> Vec<(NodeId, String)> {
    let frames = state.frames.borrow();
    let doc = state.doc.borrow();
    let Some(tree) = doc.tree(ROOT) else {
        return Vec::new();
    };
    let mut out: Vec<(NodeId, String)> = tree
        .nodes()
        .filter(|n| matches!(n.body, NodeBody::Frame))
        .filter_map(|n| frames.get(&n.id).map(|name| (n.id, name.clone())))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// The nodes inside `frame`.
fn members_of(state: &LabState, frame: NodeId) -> Vec<NodeId> {
    let doc = state.doc.borrow();
    doc.tree(ROOT).map_or_else(Vec::new, |tree| {
        tree.nodes()
            .filter(|n| n.parent == Some(frame) && !matches!(n.body, NodeBody::Frame))
            .map(|n| n.id)
            .collect()
    })
}

/// The tab strip at the top of a frame — its name, and its drag handle.
const FRAME_TAB: u32 = 18;
/// How much room a frame leaves around the cards it holds.
const FRAME_PAD: u32 = 14;

/// A host frame's rectangle, **derived from the cards it holds**.
///
/// ★ R1654 — the reference derives this (`the frame rect`, `apply frame`, `drag
/// the frame` are three of its nine frame verbs) and this screen had it as a
/// constant out of the specification table, so a frame did not grow when a card
/// was dragged into it, did not shrink when one left, and could not be moved at
/// all. Reported as "the group behaviour does not match".
///
/// A frame with no members keeps its own stored position and paints an empty
/// box, because a group you cannot see is a group you cannot drop anything into.
fn frame_rect_of(state: &LabState, frame: NodeId) -> Rect {
    let members = members_of(state, frame);
    let boxes: Vec<Rect> = members
        .iter()
        .filter_map(|n| card_rect(state, *n))
        .collect();
    if boxes.is_empty() {
        let (x, y) = state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| t.node(frame).map(|n| (n.x, n.y)))
            .unwrap_or((0, 0));
        let (cx, cy) = to_content(state, x, y);
        return Rect::new(cx, cy, scaled(state, 150), scaled(state, 90).max(FRAME_TAB));
    }
    let pad = scaled(state, FRAME_PAD).max(4);
    let tab = scaled(state, FRAME_TAB).max(10);
    let left = boxes.iter().map(|r| r.x).min().unwrap_or(0);
    let top = boxes.iter().map(|r| r.y).min().unwrap_or(0);
    let right = boxes.iter().map(|r| r.x + r.w).max().unwrap_or(0);
    let bottom = boxes.iter().map(|r| r.y + r.h).max().unwrap_or(0);
    Rect::new(
        left.saturating_sub(pad),
        top.saturating_sub(pad + tab),
        right - left + pad * 2,
        bottom - top + pad * 2 + tab,
    )
}

/// The frame whose box holds this content-space point, innermost first.
fn frame_at(state: &LabState, cx: i64, cy: i64) -> Option<NodeId> {
    frames_of(state)
        .into_iter()
        .filter(|(id, _)| holds(frame_rect_of(state, *id), cx, cy))
        .min_by_key(|(id, _)| {
            let r = frame_rect_of(state, *id);
            u64::from(r.w) * u64::from(r.h)
        })
        .map(|(id, _)| id)
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

// ── Hit testing ─────────────────────────────────────────────────────────────

/// What is under the cursor.
///
/// One enumeration read by both the press handler and the demo's sweep, from
/// the same rectangles the painter draws — the property R1648 lost by keeping a
/// second copy of the layout and R1649's two-direction sweep exists to hold.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Hit {
    Nothing,
    Rail(&'static str),
    Role(Role),
    DiscoveryToggle,
    Zoom(bool),
    Config,
    Run,
    Node(NodeId),
    Pin {
        node: NodeId,
        dial: bool,
    },
    Link(LinkId),
    /// A host frame's tab strip — its handle.
    Frame(NodeId),
    Field(String),
    AddField(String),
    /// An affordance inside a control: an option, a stepper, a checkbox, a list
    /// row. `part` is the painter's own tag suffix, so this arm covers every
    /// shape and a seventh needs no new arm here.
    Part {
        key: String,
        part: String,
    },
    Canvas,
}

impl Hit {
    fn at(state: &LabState, px: u32, py: u32) -> Self {
        // The inspector, front to back: its own geometry is the form painter's.
        if contains(inspector_rect(), px, py) {
            let geometry = inspector_geometry(state);
            for row in &geometry.rows {
                // Every affordance inside a control, from the geometry the
                // painter published — never a second layout.
                for (suffix, rect) in &row.parts {
                    if contains(*rect, px, py) {
                        return Self::Part {
                            key: row.key.clone(),
                            part: suffix.clone(),
                        };
                    }
                }
                if contains(row.control, px, py) || contains(row.header, px, py) {
                    return Self::Field(row.key.clone());
                }
            }
            for (key, rect) in &geometry.chips {
                if contains(*rect, px, py) {
                    return Self::AddField(key.clone());
                }
            }
            return Self::Nothing;
        }
        if contains(rail_rect(), px, py) {
            for (n, (name, _)) in spec::RAIL.iter().enumerate() {
                if contains(rail_seat(n), px, py) {
                    return Self::Rail(name);
                }
            }
            return Self::Nothing;
        }
        if contains(palette_rect(), px, py) {
            for (n, role) in Role::ALL.into_iter().enumerate() {
                if contains(palette_row(n), px, py) {
                    return Self::Role(role);
                }
            }
            if contains(discovery_rect(), px, py) {
                return Self::DiscoveryToggle;
            }
            return Self::Nothing;
        }
        if contains(toolbar_rect(), px, py) {
            if contains(zoom_rect(false), px, py) {
                return Self::Zoom(false);
            }
            if contains(zoom_rect(true), px, py) {
                return Self::Zoom(true);
            }
            if contains(config_rect(), px, py) {
                return Self::Config;
            }
            if contains(run_rect(), px, py) {
                return Self::Run;
            }
            return Self::Nothing;
        }
        if contains(canvas_rect(), px, py) {
            // ★ The canvas is a viewport onto a world surface, so a press is
            // resolved in the surface's coordinates — the same ones the painter
            // places cards in. One conversion, at the boundary.
            let (cx, cy) = window_to_content(state, px, py);
            // Pins before cards: a pin overhangs its card's edge, and the pin
            // is the smaller target, so testing the card first would make a
            // link impossible to author with a real mouse.
            for node in state.cards() {
                let Some(card) = card_rect(state, node) else {
                    continue;
                };
                if holds(pin_rect(state, card, true), cx, cy) {
                    return Self::Pin { node, dial: true };
                }
                if state.role_of(node).is_some_and(Role::accepts)
                    && holds(pin_rect(state, card, false), cx, cy)
                {
                    return Self::Pin { node, dial: false };
                }
            }
            for node in state.cards().into_iter().rev() {
                if card_rect(state, node).is_some_and(|r| holds(r, cx, cy)) {
                    return Self::Node(node);
                }
            }
            if let Some(link) = link_at(state, cx, cy) {
                return Self::Link(link);
            }
            // The frame's TAB, not its interior: the interior is where the
            // cards are, and a group that swallowed presses over its own
            // members would make a node undraggable the moment it joined one.
            for (id, _) in frames_of(state) {
                let r = frame_rect_of(state, id);
                let tab = Rect::new(r.x, r.y, r.w, scaled(state, FRAME_TAB).max(10));
                if holds(tab, cx, cy) {
                    return Self::Frame(id);
                }
            }
            return Self::Canvas;
        }
        Self::Nothing
    }

    /// The word the wire answers a press with.
    fn word(&self, state: &LabState) -> String {
        match self {
            Self::Nothing => "nothing".into(),
            Self::Rail(name) => format!("rail:{name}"),
            Self::Role(role) => format!("role:{}", role.name()),
            Self::DiscoveryToggle => "discovery".into(),
            Self::Zoom(up) => format!("zoom:{}", if *up { "in" } else { "out" }),
            Self::Config => "config".into(),
            Self::Run => "run".into(),
            Self::Node(id) => format!("node:{}", state.name_of(*id)),
            Self::Pin { node, dial } => format!(
                "pin:{}:{}",
                state.name_of(*node),
                if *dial { "dial" } else { "accept" }
            ),
            Self::Link(id) => format!("link:{}", id.0),
            Self::Frame(id) => format!(
                "frame:{}",
                state.frames.borrow().get(id).cloned().unwrap_or_default()
            ),
            Self::Field(key) => format!("field:{key}"),
            Self::AddField(key) => format!("add:{key}"),
            Self::Part { part, .. } => part.clone(),
            Self::Canvas => "canvas".into(),
        }
    }
}

/// The link whose wire passes within a few pixels of the cursor, in the world
/// surface's own coordinates.
fn link_at(state: &LabState, px: i64, py: i64) -> Option<LinkId> {
    let doc = state.doc.borrow();
    let tree = doc.tree(ROOT)?;
    for link in tree.links() {
        let (Some(a), Some(b)) = (
            card_rect(state, link.from.node),
            card_rect(state, link.to.node),
        ) else {
            continue;
        };
        let (ax, ay) = centre(pin_rect(state, a, true));
        let (bx, by) = centre(pin_rect(state, b, false));
        // Sample the straight chord: the wire is drawn as a curve between the
        // same two points, and the chord is within the tolerance a finger has.
        for step in 0..=20u32 {
            let t = f64::from(step) / 20.0;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a lerp between two pixels is a pixel"
            )]
            let (lx, ly) = (
                (f64::from(ax) + (f64::from(bx) - f64::from(ax)) * t) as i64,
                (f64::from(ay) + (f64::from(by) - f64::from(ay)) * t) as i64,
            );
            if px.abs_diff(lx) <= 6 && py.abs_diff(ly) <= 6 {
                return Some(link.id);
            }
        }
    }
    None
}

// ── Chrome rectangles ───────────────────────────────────────────────────────

fn rail_seat(n: usize) -> Rect {
    Rect::new(
        8,
        APP_BAR_H + 10 + u32::try_from(n).unwrap_or(0) * 42,
        38,
        38,
    )
}

fn palette_row(n: usize) -> Rect {
    let n = u32::try_from(n).unwrap_or(0);
    // Two groups of four, each under its own heading.
    let group = n / 4;
    let within = n % 4;
    Rect::new(
        RAIL_W + PAD,
        APP_BAR_H
            + 56
            + group * (PAL_HEAD_H + 4 * PAL_ROW_H + 12)
            + PAL_HEAD_H
            + within * PAL_ROW_H,
        PALETTE_W - PAD * 2,
        PAL_ROW_H - 5,
    )
}

/// Where the pin legend starts — under both palette groups.
fn legend_top() -> u32 {
    APP_BAR_H + 56 + 2 * (PAL_HEAD_H + 4 * PAL_ROW_H + 12)
}

fn legend_row(n: usize) -> Rect {
    Rect::new(
        RAIL_W + PAD,
        legend_top() + PAL_HEAD_H + u32::try_from(n).unwrap_or(0) * 20,
        PALETTE_W - PAD * 2,
        18,
    )
}

fn protocol_chip(n: usize) -> Rect {
    Rect::new(
        RAIL_W + PAD + u32::try_from(n).unwrap_or(0) * 40,
        legend_top() + PAL_HEAD_H + 3 * 20 + 6,
        36,
        18,
    )
}

fn discovery_rect() -> Rect {
    Rect::new(
        RAIL_W + PAD,
        legend_top() + PAL_HEAD_H + 3 * 20 + 6 + 18 + 20 + PAL_HEAD_H,
        PALETTE_W - PAD * 2,
        58,
    )
}

fn zoom_rect(plus: bool) -> Rect {
    let bar = toolbar_rect();
    let right = bar.x + bar.w;
    Rect::new(
        if plus { right - 232 } else { right - 300 },
        bar.y + 11,
        24,
        24,
    )
}

fn config_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(bar.x + bar.w - 196, bar.y + 9, 66, 28)
}

fn run_rect() -> Rect {
    let bar = toolbar_rect();
    Rect::new(bar.x + bar.w - 120, bar.y + 9, 106, 28)
}

/// The launch gate panel, bottom right of the canvas.
fn gate_rect(state: &LabState) -> Rect {
    let canvas = canvas_rect();
    let lines = u32::try_from(state.gate_lines().len()).unwrap_or(0) + 1;
    let h = 34 + lines * 20;
    Rect::new(
        canvas.x + canvas.w - 262,
        canvas.y + canvas.h - h - 12,
        250,
        h,
    )
}

fn hint_rect() -> Rect {
    let canvas = canvas_rect();
    Rect::new(canvas.x + 12, canvas.y + canvas.h - 34, 470, 24)
}

// ── The inspector ───────────────────────────────────────────────────────────

fn selected_form(state: &LabState) -> Option<ConfigForm> {
    let node = state.selected.get()?;
    state.forms.borrow().get(&node).cloned()
}

/// Where the inspector's form is laid out.
///
/// `WrapAll` + `AllGrow`, which is the reference inspector's own choice and the
/// right one for the reason its own screen shows: a configuration path is long,
/// and a key column wide enough for `transport.link.tx.batch_size` would leave
/// no room for its value.
fn form_style() -> FormStyle {
    FormStyle::default()
        .with_width(INSP_W - PAD * 2)
        .with_policy(RowWrap::WrapAll, FieldGrowth::AllGrow)
}

/// Where the inspector's identity block ends and its form begins.
const INSP_HEAD_H: u32 = 118;

fn inspector_geometry(state: &LabState) -> FormGeometry {
    let form = selected_form(state).unwrap_or_default();
    let rect = inspector_rect();
    form_geometry(&form, (rect.x + PAD, rect.y + INSP_HEAD_H), &form_style())
}

// ── Paint helpers ───────────────────────────────────────────────────────────

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

/// A text run at an exact rectangle inside its container.
///
/// ★★ **The layout style is load-bearing and its absence is the defect R1653
/// found.** A [`TextNode`] carries a `rect`, which reads like a position and is
/// not one: with no [`LayoutStyle`] the engine treats the run as a flow child
/// and stacks it under its predecessor, so a screen that computes every
/// rectangle correctly still paints its text in a column down the left edge of
/// whatever contains it. `hello-analyzer-shell` learned this at R1649 and this
/// example was written afterwards without it — every label here flowed, all of
/// the canvas's card text landed in one stripe, and six rounds of gates passed
/// anyway because a run carries no tag and every gate was tag-keyed.
fn label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(text.into(), rect, run_style(px, fg)).with_layout(absolute(rect)))
}

fn tagged_label(tag: &str, text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(text.into(), rect, run_style(px, fg))
            .with_tag(tag.to_owned())
            .with_layout(absolute(rect)),
    )
}

/// The style every run on this screen carries.
///
/// ★ R1654 — including an overflow policy, because the box is exact and the
/// content is not: an endpoint, a key expression and a node identifier are all
/// user data, and a run wider than the box it was given wraps to a second line
/// that lands on the row below. Two rounds of this screen shipped that smear —
/// R1653 gave every run its exact box and could not see the overflow, because
/// its check measures boxes and this is about glyphs.
///
/// `Ellipsis` rather than `Clip` for the same reason a person reads it: a hard
/// cut leaves no evidence that anything was removed, so `tcp/0.0.0.0:744` and
/// `tcp/0.0.0.0:7447` are indistinguishable on screen.
fn run_style(px: u32, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

/// A run whose content is a PATH, where the tail is what distinguishes one from
/// another — so the middle gives way rather than the end.
fn path_style(px: u32, fg: Color) -> TextStyle {
    run_style(px, fg).with_overflow(TextOverflow::EllipsisMiddle)
}

/// A value cell on a node card: the right-hand column of a digest row.
fn value_label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(text.into(), rect, path_style(px, fg)).with_layout(absolute(rect)))
}

fn panel(tag: &str, rect: Rect, fill: Color, border: Option<Color>, children: Vec<Scene>) -> Scene {
    let mut style = BoxStyle::filled(fill);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, 1));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

fn box_at(tag: &str, rect: Rect, fill: Color, border: Option<Color>, radius: u32) -> Scene {
    let mut style = BoxStyle::filled(fill).with_corner_radius(radius);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, 1));
    }
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

/// A wire between two pins, drawn as the reference draws it: a horizontal-ease
/// cubic, so a link leaves a dial pin going right and arrives at an accept pin
/// coming from the left whatever the vertical distance.
fn wire(tag: &str, from: (u32, u32), to: (u32, u32), colour: Color, width: u32) -> Scene {
    let fx = f32::from(u16::try_from(from.0).unwrap_or(u16::MAX));
    let fy = f32::from(u16::try_from(from.1).unwrap_or(u16::MAX));
    let tx = f32::from(u16::try_from(to.0).unwrap_or(u16::MAX));
    let ty = f32::from(u16::try_from(to.1).unwrap_or(u16::MAX));
    let bow = ((tx - fx).abs() * 0.5).max(24.0);
    let bounds = Rect::new(
        from.0.min(to.0),
        from.1.min(to.1),
        from.0.abs_diff(to.0).max(1),
        from.1.abs_diff(to.1).max(1),
    );
    Scene::Path(
        PathNode::new(
            bounds,
            vec![
                PathCommand::MoveTo(PathPoint::new(fx, fy)),
                PathCommand::CurveTo {
                    c1: PathPoint::new(fx + bow, fy),
                    c2: PathPoint::new(tx - bow, ty),
                    end: PathPoint::new(tx, ty),
                },
            ],
            PathStyle::stroked(Stroke::new(colour, width)),
        )
        .with_tag(tag.to_owned())
        // ★ R1655 — a wire's BOUNDING BOX is most of the canvas, and a tagged
        // node that is not transparent is what the §5.35 router resolves as the
        // hit target: it looks the tag up as an `External`, finds none, and
        // forwards nothing. Measured by reverting it: with the wires opaque the
        // app never received the cursor at all (it reported 0,0 after a real
        // warp onto the canvas), which is exactly "sometimes a node presses and
        // sometimes it does not". The link stays selectable — `link_at`
        // hit-tests the CHORD in the app's own resolver, which is where a
        // wire's shape lives.
        .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
    )
}

// ── The view ────────────────────────────────────────────────────────────────

fn app_bar(state: &LabState, ink: Ink) -> Scene {
    let running = state.running.get();
    panel(
        "lab.appbar",
        Rect::new(0, 0, window_size().0, APP_BAR_H),
        ink.surface,
        Some(ink.outline),
        vec![
            label("node lab", Rect::new(16, 19, 90, 16), FONT_TITLE, ink.text),
            tagged_label(
                "lab.appbar.graph",
                spec::GRAPH_NAME,
                Rect::new(118, 20, 200, 14),
                FONT_SMALL,
                ink.text_2,
            ),
            tagged_label(
                "lab.appbar.state",
                if running { "running" } else { "stopped" },
                Rect::new(window_size().0 - 120, 20, 100, 14),
                FONT_SMALL,
                if running { ink.ok } else { ink.text_3 },
            ),
        ],
    )
}

fn rail(ink: Ink) -> Scene {
    let rect = rail_rect();
    // ★ R1651.1 — LOCAL coordinates. These seats were painted at their window
    // rectangles inside a container already placed at `rail_rect()`, so every
    // one of them drew a pane-height below where the hit test looks and a press
    // on `packets` answered `keys`. R1648's double-offset defect, again, in the
    // one pane R1651's sweep did not probe.
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
    let mut children = vec![];
    for (n, (name, locked)) in spec::RAIL.iter().enumerate() {
        let seat = local(rail_seat(n));
        let active = *name == spec::RAIL_ACTIVE;
        children.push(box_at(
            &format!("lab.rail.{name}"),
            seat,
            if active { ink.accent_soft } else { ink.surface },
            Some(if active { ink.accent_line } else { ink.surface }),
            10,
        ));
        children.extend(rail_icon(
            name,
            seat,
            if active {
                ink.accent
            } else if *locked {
                ink.text_3
            } else {
                ink.text_2
            },
        ));
    }
    panel("lab.rail", rect, ink.surface, Some(ink.outline), children)
}

/// A rail seat's icon, drawn as marks rather than written as a character.
///
/// ★ R1654 — these were a `\u{2022}` and a `\u{00B7}` in a 12px face, which is
/// a dot and a smaller dot: seven destinations that a reader cannot tell apart,
/// reported from the running window as "the icons are not visible". A glyph
/// font is not an option here (vendoring one is forbidden in this tree), so the
/// marks are composed from the primitives the scene already has, and each one
/// says what its destination IS: a board of tiles, a stack of messages, a key,
/// lines of a log, two joined nodes, a hub with spokes, a session's two panes.
fn rail_icon(name: &str, seat: Rect, ink: Color) -> Vec<Scene> {
    let (ox, oy) = (seat.x + 11, seat.y + 11);
    let pip = |x: u32, y: u32, w: u32, h: u32| {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_style(BoxStyle::filled(ink).with_corner_radius(1))
                .with_layout(absolute(Rect::new(ox + x, oy + y, w, h))),
        )
    };
    let ring = |x: u32, y: u32, d: u32| {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_style(
                    BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                        .with_border(Border::new(ink, 1))
                        .with_corner_radius(d / 2),
                )
                .with_layout(absolute(Rect::new(ox + x, oy + y, d, d))),
        )
    };
    match name {
        // A board of tiles.
        "dashboard" => vec![
            pip(0, 0, 7, 7),
            pip(9, 0, 7, 7),
            pip(0, 9, 7, 7),
            pip(9, 9, 7, 7),
        ],
        // A stack of messages, the top one shorter because it is the newest.
        "packets" => vec![pip(0, 1, 10, 3), pip(0, 7, 16, 3), pip(0, 13, 13, 3)],
        // A key: the bow, the shaft, two teeth.
        "keys" => vec![
            ring(0, 4, 8),
            pip(8, 7, 8, 2),
            pip(12, 9, 2, 4),
            pip(15, 9, 1, 3),
        ],
        // Lines of a log, ragged as text is.
        "logs" => vec![pip(0, 1, 16, 2), pip(0, 6, 11, 2), pip(0, 11, 14, 2)],
        // Two nodes and the link between them — this screen.
        "lab" => vec![ring(0, 0, 7), ring(9, 9, 7), pip(6, 6, 5, 2)],
        // A hub with three spokes.
        "topology" => vec![
            ring(5, 5, 7),
            pip(0, 8, 5, 1),
            pip(12, 8, 4, 1),
            pip(8, 12, 1, 4),
        ],
        // Two panes of one session.
        _ => vec![pip(0, 0, 7, 16), pip(9, 0, 7, 16)],
    }
}

fn palette(state: &LabState, ink: Ink) -> Scene {
    let rect = palette_rect();
    let mut children = vec![
        label(
            spec::PANES[1].title,
            Rect::new(PAD, 14, 180, 16),
            FONT_BODY + 1,
            ink.text,
        ),
        label(
            "click to add one at the centre",
            Rect::new(PAD, 34, 200, 12),
            10,
            ink.text_3,
        ),
    ];
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);

    for (group_n, group) in ["infrastructure", "traffic"].into_iter().enumerate() {
        let head = palette_row(group_n * 4);
        children.push(label(
            group,
            Rect::new(head.x - rect.x, head.y - rect.y - PAL_HEAD_H, 180, 12),
            10,
            ink.text_3,
        ));
    }
    for (n, role) in Role::ALL.into_iter().enumerate() {
        let row = local(palette_row(n));
        children.push(box_at(
            &format!("lab.palette.role.{}", role.name()),
            row,
            ink.raised,
            Some(ink.outline),
            8,
        ));
        children.push(box_at(
            &format!("lab.palette.swatch.{}", role.name()),
            Rect::new(row.x + 9, row.y + 6, 3, row.h - 12),
            role_ink(role),
            None,
            2,
        ));
        children.push(label(
            role.name(),
            Rect::new(row.x + 20, row.y + 6, 140, 14),
            FONT_SMALL + 1,
            ink.text,
        ));
        children.push(label(
            role.gist(),
            Rect::new(row.x + 20, row.y + 20, 160, 12),
            10,
            ink.text_3,
        ));
    }

    children.extend(palette_legend(rect, ink));
    children.extend(palette_determinism(state, rect, ink));
    panel(
        "lab.palette",
        rect,
        ink.surface,
        Some(ink.outline),
        children,
    )
}

/// The pin legend and the transport chips: three appearances and what each one
/// means, next to the colours an accept pin is drawn in.
fn palette_legend(rect: Rect, ink: Ink) -> Vec<Scene> {
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
    let mut children = vec![label(
        "pins",
        Rect::new(PAD, legend_top() - rect.y, 120, 12),
        10,
        ink.text_3,
    )];
    for (n, (kind, meaning)) in spec::PIN_LEGEND.iter().enumerate() {
        let row = local(legend_row(n));
        let colour = match *kind {
            "dial" => ink.accent,
            "accept" => transport_ink(Transport::Tcp),
            _ => ink.text_3,
        };
        children.push(box_at(
            &format!("lab.palette.pin.{kind}"),
            Rect::new(row.x, row.y + 3, PIN, PIN),
            if *kind == "dial" { colour } else { ink.surface },
            Some(colour),
            PIN / 2,
        ));
        children.push(label(
            *meaning,
            Rect::new(row.x + 20, row.y + 3, 190, 12),
            10,
            if *kind == "closed" {
                ink.err
            } else {
                ink.text_2
            },
        ));
    }
    for (n, transport) in Transport::ALL.into_iter().enumerate() {
        let chip = local(protocol_chip(n));
        children.push(box_at(
            &format!("lab.palette.protocol.{}", transport.word()),
            chip,
            ink.surface,
            Some(transport_ink(transport)),
            4,
        ));
        children.push(label(
            transport.word(),
            Rect::new(chip.x + 7, chip.y + 4, 32, 12),
            10,
            transport_ink(transport),
        ));
    }

    children
}

/// The determinism switch, off by default.
fn palette_determinism(state: &LabState, rect: Rect, ink: Ink) -> Vec<Scene> {
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
    let mut children: Vec<Scene> = Vec::new();
    let toggle = local(discovery_rect());
    let on = state.discovery.get();
    children.push(label(
        "graph determinism",
        Rect::new(toggle.x, toggle.y - PAL_HEAD_H, 180, 12),
        10,
        ink.text_3,
    ));
    children.push(box_at(
        "lab.palette.discovery",
        toggle,
        ink.raised,
        Some(if on { ink.warn } else { ink.outline }),
        9,
    ));
    children.push(box_at(
        "lab.palette.discovery.track",
        Rect::new(toggle.x + 10, toggle.y + 12, 30, 16),
        if on { ink.warn } else { ink.outline_2 },
        None,
        8,
    ));
    children.push(tagged_label(
        "lab.palette.discovery.state",
        if on {
            "discovery on · links may appear"
        } else {
            "discovery off · fully specified"
        },
        Rect::new(
            toggle.x + 48,
            toggle.y + 10,
            PALETTE_W - toggle.x - 48 - PAD,
            13,
        ),
        FONT_SMALL,
        ink.text,
    ));
    children.push(label(
        "turning it on lets nodes acquire links nobody authored",
        Rect::new(
            toggle.x + 48,
            toggle.y + 28,
            PALETTE_W - toggle.x - 48 - PAD,
            24,
        ),
        9,
        ink.text_2,
    ));

    children
}

fn toolbar(state: &LabState, ink: Ink) -> Scene {
    /// The clearance between one toolbar label's box and the next one's.
    const GAP: u32 = 8;
    let bar = toolbar_rect();
    let verdict = state.verdict();
    let nodes = state.cards().len();
    let links = state.link_count();

    let gate_word = if verdict.may_launch() {
        "gate passed"
    } else {
        "gate blocked"
    };
    let gate_colour = if verdict.may_launch() {
        ink.ok
    } else {
        ink.err
    };

    // ★ R1653 — each label's box ends where the next one begins. A text node's
    // rectangle is not a hint, it is the box the run is wrapped into: two boxes
    // that overlap paint one string over another as soon as either string grows
    // to fill the space it was promised, and the strings here are a graph name
    // and a count.
    let mut children = vec![
        tagged_label(
            "lab.toolbar.title",
            spec::GRAPH_NAME,
            Rect::new(PAD, 15, 160 - GAP, 16),
            FONT_TITLE,
            ink.text,
        ),
        tagged_label(
            "lab.toolbar.meta",
            format!("{nodes} nodes · {links} links"),
            Rect::new(PAD + 160, 17, 140 - GAP, 13),
            FONT_SMALL,
            ink.text_3,
        ),
        // The word lives INSIDE the chip rather than beside it, so it is the
        // chip's own content and cannot drift out of it.
        panel(
            "lab.toolbar.gate",
            Rect::new(PAD + 300, 12, 104, 22),
            ink.raised,
            Some(gate_colour),
            vec![label(
                gate_word,
                Rect::new(10, 5, 84, 13),
                FONT_SMALL,
                gate_colour,
            )],
        ),
    ];

    children.extend(toolbar_controls(state, ink));
    panel("lab.toolbar", bar, ink.surface, Some(ink.outline), children)
}

/// The toolbar's right-hand cluster: zoom, the configuration read-out, and the
/// run control the gate governs.
fn toolbar_controls(state: &LabState, ink: Ink) -> Vec<Scene> {
    let bar = toolbar_rect();
    let local = |r: Rect| Rect::new(r.x - bar.x, r.y - bar.y, r.w, r.h);
    let verdict = state.verdict();
    let running = state.running.get();
    let nodes = state.cards().len();
    let mut children: Vec<Scene> = Vec::new();
    for plus in [false, true] {
        let seat = local(zoom_rect(plus));
        children.push(box_at(
            if plus {
                "lab.toolbar.zoom.in"
            } else {
                "lab.toolbar.zoom.out"
            },
            seat,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        children.push(label(
            if plus { "+" } else { "-" },
            Rect::new(seat.x + 9, seat.y + 5, 12, 14),
            FONT_BODY,
            ink.text,
        ));
    }
    children.push(tagged_label(
        "lab.toolbar.zoom",
        format!("{}%", state.zoom.get()),
        Rect::new(local(zoom_rect(false)).x + 30, 17, 40, 13),
        FONT_SMALL,
        ink.text,
    ));

    let config = local(config_rect());
    children.push(box_at(
        "lab.toolbar.config",
        config,
        ink.raised,
        Some(ink.outline),
        7,
    ));
    children.push(label(
        "config",
        Rect::new(config.x + 12, config.y + 8, 60, 13),
        FONT_SMALL,
        ink.text_2,
    ));

    let run = local(run_rect());
    let run_ink = if !verdict.may_launch() {
        ink.text_3
    } else if running {
        ink.ok
    } else {
        ink.accent
    };
    children.push(box_at("lab.toolbar.run", run, ink.raised, Some(run_ink), 7));
    children.push(tagged_label(
        "lab.toolbar.run.label",
        if running {
            format!("running {nodes}/{nodes}")
        } else if verdict.may_launch() {
            "run".to_string()
        } else {
            "run blocked".to_string()
        },
        Rect::new(run.x + 12, run.y + 8, 90, 13),
        FONT_SMALL,
        run_ink,
    ));
    children
}

fn canvas_world(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();

    children.extend(canvas_grid(state, ink));

    let dragged_frame = match state.drag.get() {
        Some(Drag::Frame { frame, .. }) => Some(frame),
        _ => None,
    };
    for (id, name) in frames_of(state) {
        let box_rect = frame_rect_of(state, id);
        let gist = spec::FRAMES
            .iter()
            .find(|f| f.name == name)
            .map_or("", |f| f.gist);
        children.push(box_at(
            &format!("lab.frame.{name}"),
            box_rect,
            Color::rgba(0x16, 0x18, 0x1D, 0x6b),
            Some(if dragged_frame == Some(id) {
                ink.accent
            } else {
                ink.outline_2
            }),
            12,
        ));
        // The tab is the frame's handle: the interior belongs to the cards, so
        // a group can be moved without a press inside it stealing a node drag.
        children.push(tagged_label(
            &format!("lab.frame.{name}.name"),
            if gist.is_empty() {
                name.clone()
            } else {
                format!("{name} · {gist}")
            },
            Rect::new(
                box_rect.x + 12,
                box_rect.y + 3,
                box_rect.w.saturating_sub(24).max(40),
                13,
            ),
            10,
            ink.text_3,
        ));
    }
    children.extend(canvas_wires(state, ink));
    children.extend(canvas_cards(state, ink));
    children
}

/// The reference's dot grid: a pip every 22 canvas units, moving with the pan
/// so the canvas reads as a surface being moved rather than a viewport sliding
/// over a static picture.
fn canvas_grid(state: &LabState, ink: Ink) -> Vec<Scene> {
    let rect = canvas_rect();
    let mut children: Vec<Scene> = Vec::new();
    let pitch = scaled(state, 22).max(6);
    // Only the slice of the surface the viewport is over: the pips are a
    // texture, and a texture over the whole 6,400-unit world would be a quarter
    // of a million nodes to lay out for the few thousand anybody can see.
    let (ox, oy) = world_offset(state, state.pan.get());
    let first = |offset: i32| {
        let pitch = i32::try_from(pitch).unwrap_or(1);
        u32::try_from(offset - offset.rem_euclid(pitch)).unwrap_or(0)
    };
    let (from_x, from_y) = (first(ox), first(oy));
    let mut gy = from_y;
    while gy < from_y + rect.h + pitch {
        let mut gx = from_x;
        while gx < from_x + rect.w + pitch {
            children.push(Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_style(BoxStyle::filled(ink.grid))
                    .with_layout(absolute(Rect::new(gx, gy, 1, 1))),
            ));
            gx += pitch;
        }
        gy += pitch;
    }

    children
}

/// The wires, and the label the selected one alone carries.
fn canvas_wires(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();
    let selected_link = state.selected_link.get();
    {
        let doc = state.doc.borrow();
        if let Some(tree) = doc.tree(ROOT) {
            for link in tree.links() {
                let (Some(a), Some(b)) = (
                    card_rect(state, link.from.node),
                    card_rect(state, link.to.node),
                ) else {
                    continue;
                };
                let chosen = selected_link == Some(link.id);
                let from = centre(pin_rect(state, a, true));
                let to = centre(pin_rect(state, b, false));
                children.push(wire(
                    &format!("lab.link.{}", link.id.0),
                    from,
                    to,
                    if chosen { ink.accent } else { ink.accent_line },
                    if chosen { 2 } else { 1 },
                ));
                if chosen {
                    let endpoint = state
                        .forms
                        .borrow()
                        .get(&link.to.node)
                        .and_then(|f| f.field("listen.endpoints").map(|v| v.value().to_owned()))
                        .unwrap_or_default();
                    let mid = (u32::midpoint(from.0, to.0), u32::midpoint(from.1, to.1));
                    // ★ The text is the label's CHILD, not its neighbour. A
                    // floating annotation is drawn over the diagram on purpose,
                    // so it is its own layer, and saying that structurally is
                    // what keeps "no two runs of one widget overlap" true
                    // without an exception list beside the rule.
                    children.push(panel(
                        "lab.link.label",
                        Rect::new(mid.0.saturating_sub(70), mid.1.saturating_sub(24), 150, 20),
                        ink.accent_soft,
                        Some(ink.accent_line),
                        vec![tagged_label(
                            "lab.link.label.text",
                            endpoint,
                            Rect::new(7, 6, 136, 13),
                            10,
                            ink.accent,
                        )],
                    ));
                }
            }
        }
    }

    // A link being authored follows the cursor, so a drag shows what it will
    // make before it makes it — the reference commits on release.
    if let Some(Drag::Wire { from }) = state.drag.get() {
        if let Some(card) = card_rect(state, from) {
            let cursor = state.cursor.get();
            let (cx, cy) = window_to_content(state, cursor.0, cursor.1);
            children.push(wire(
                "lab.link.preview",
                centre(pin_rect(state, card, true)),
                (
                    u32::try_from(cx).unwrap_or(0),
                    u32::try_from(cy).unwrap_or(0),
                ),
                ink.accent,
                2,
            ));
        }
    }

    children
}

/// One card per node: its identity band, its digest rows, and its pins.
fn canvas_cards(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();
    let selected = state.selected.get();
    for node in state.cards() {
        let Some(card) = card_rect(state, node) else {
            continue;
        };
        let name = state.name_of(node);
        let rows = card_rows(state, node);
        let role = state.role_of(node).unwrap_or(Role::Peer);
        let chosen = selected == Some(node);
        let card_local = card;
        children.push(box_at(
            &format!("lab.node.{name}"),
            card_local,
            ink.surface,
            Some(if chosen { ink.accent } else { ink.outline_2 }),
            9,
        ));
        children.push(tagged_label(
            &format!("lab.node.{name}.id"),
            name.clone(),
            Rect::new(card_local.x + 10, card_local.y + 7, 60, 13),
            FONT_SMALL,
            ink.text,
        ));
        children.push(box_at(
            &format!("lab.node.{name}.badge"),
            Rect::new(
                card_local.x + card_local.w.saturating_sub(46),
                card_local.y + 6,
                38,
                14,
            ),
            ink.surface,
            Some(role_ink(role)),
            4,
        ));
        children.push(label(
            role.badge(),
            Rect::new(
                card_local.x + card_local.w.saturating_sub(42),
                card_local.y + 8,
                34,
                11,
            ),
            8,
            role_ink(role),
        ));
        for (n, (key, value)) in rows.iter().enumerate() {
            let y = card_local.y + CARD_HDR + u32::try_from(n).unwrap_or(0) * CARD_ROW_H;
            children.push(label(
                key.clone(),
                Rect::new(card_local.x + 10, y, 40, 11),
                FONT_TINY,
                ink.text_3,
            ));
            // The value column holds user data — an endpoint, a key
            // expression — and its TAIL is what distinguishes one from another,
            // so the middle gives way rather than the end.
            children.push(value_label(
                value.clone(),
                Rect::new(card_local.x + 52, y, card_local.w.saturating_sub(60), 11),
                FONT_TINY,
                ink.text_2,
            ));
        }

        children.extend(canvas_pins(state, node, card, role, ink));
    }
    children
}

/// A node's pins. Their appearance **is** the rule the legend states: filled =
/// can dial, ringed in the transport's colour = can be dialled, grey = the role
/// listens and this node has nowhere to.
fn canvas_pins(state: &LabState, node: NodeId, card: Rect, role: Role, ink: Ink) -> Vec<Scene> {
    let name = state.name_of(node);
    let mut children: Vec<Scene> = Vec::new();
    {
        let listening = state.forms.borrow().get(&node).is_some_and(|f| {
            f.field("listen.endpoints")
                .is_some_and(|v| !v.value().trim().is_empty())
        });
        let transport = state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| t.node(node))
            .and_then(|n| match &n.body {
                NodeBody::Kind(kind) => Some(kind.transport),
                _ => None,
            })
            .unwrap_or(Transport::Tcp);
        children.push(box_at(
            &format!("lab.pin.{name}.dial"),
            pin_rect(state, card, true),
            ink.accent,
            Some(ink.accent),
            PIN / 2,
        ));
        if role.accepts() {
            children.push(box_at(
                &format!("lab.pin.{name}.accept"),
                pin_rect(state, card, false),
                ink.surface,
                Some(if listening {
                    transport_ink(transport)
                } else {
                    ink.text_3
                }),
                PIN / 2,
            ));
        }
    }
    children
}

/// The two things that float over the canvas: the launch gate and the gesture
/// hint.
fn canvas_overlays(state: &LabState, ink: Ink) -> Vec<Scene> {
    let rect = canvas_rect();
    let local = |r: Rect| Rect::new(r.x - rect.x, r.y - rect.y, r.w, r.h);
    let mut children: Vec<Scene> = Vec::new();
    let gate = local(gate_rect(state));
    let verdict = state.verdict();
    children.push(box_at(
        "lab.gate",
        gate,
        ink.surface,
        Some(ink.outline_2),
        10,
    ));
    children.push(tagged_label(
        "lab.gate.head",
        "pre-launch check",
        Rect::new(gate.x + 12, gate.y + 10, 150, 13),
        FONT_SMALL,
        ink.text,
    ));
    children.push(tagged_label(
        "lab.gate.verdict",
        verdict.sentence(),
        Rect::new(gate.x + 12, gate.y + 28, gate.w - 24, 13),
        9,
        if verdict.may_launch() {
            ink.ok
        } else {
            ink.err
        },
    ));
    for (n, (blocks, sentence)) in state.gate_lines().iter().enumerate() {
        children.push(tagged_label(
            &format!("lab.gate.line.{n}"),
            sentence.clone(),
            Rect::new(
                gate.x + 12,
                gate.y + 48 + u32::try_from(n).unwrap_or(0) * 20,
                gate.w - 24,
                13,
            ),
            9,
            if *blocks { ink.err } else { ink.warn },
        ));
    }

    let hint = local(hint_rect());
    children.push(box_at("lab.hint", hint, ink.surface, Some(ink.outline), 8));
    children.push(tagged_label(
        "lab.hint.text",
        spec::GESTURES
            .iter()
            .map(|(g, what)| format!("{g} = {what}"))
            .collect::<Vec<_>>()
            .join(" · "),
        Rect::new(hint.x + 10, hint.y + 6, hint.w - 20, 13),
        9,
        ink.text_3,
    ));

    children
}

/// The canvas pane: its layers, in the order a reader meets them — the surface,
/// the host frames, the wires, the node cards, then the two things that float
/// over all of it.
fn canvas(state: &LabState, ink: Ink) -> Scene {
    let rect = canvas_rect();
    // ★ R1653 — the world surface, and the viewport the pan slides over it.
    // The alternative the screen shipped with was to add the pan to every
    // rectangle: that has no clip (panned content painted over the palette and
    // the inspector) and it underflows on a leftward pan, which is a crash on
    // half of the gesture the hint strip advertises.
    let world = Scene::Container(
        ContainerNode::new(canvas_world(state, ink))
            .with_style(BoxStyle::filled(ink.bg))
            .with_layout(LayoutStyle::new().with_size(Size::px(
                scaled(state, WORLD.unsigned_abs()).max(rect.w),
                scaled(state, WORLD.unsigned_abs()).max(rect.h),
            ))),
    );
    let (ox, oy) = world_offset(state, state.pan.get());
    let viewport = Scene::Scroll(
        ScrollNode::new(Rect::new(0, 0, rect.w, rect.h), world)
            .with_axis(ScrollAxis::Both)
            .with_offset(ox, oy)
            // A drag on this surface is a pan, not a selection sweep, so the
            // edge must not carry the content away under the cursor.
            .with_auto_scroll(AutoScroll::off()),
    );
    let mut children = vec![viewport];
    // The gate panel and the hint strip are chrome: they float over the canvas
    // and do not pan with it.
    children.extend(canvas_overlays(state, ink));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("lab.canvas")
            .with_style(BoxStyle::filled(ink.bg))
            .with_layout(absolute(rect)),
    )
}

fn inspector(state: &LabState, theme: &Theme, ink: Ink) -> Scene {
    let rect = inspector_rect();
    let mut children = vec![label(
        spec::PANES[3].title,
        Rect::new(PAD, 14, 180, 16),
        FONT_BODY + 1,
        ink.text,
    )];

    let Some(node) = state.selected.get() else {
        children.push(label(
            "no node selected",
            Rect::new(PAD, 48, 200, 14),
            FONT_SMALL,
            ink.text_3,
        ));
        return panel(
            "lab.inspector",
            rect,
            ink.surface,
            Some(ink.outline),
            children,
        );
    };
    children.extend(inspector_identity(state, node, ink));
    inspector_pane(state, theme, ink, children)
}

/// Who the inspected node is: its identifier, its role and frame, and how many
/// links reach it.
fn inspector_identity(state: &LabState, node: NodeId, ink: Ink) -> Vec<Scene> {
    let name = state.name_of(node);
    let role = state.role_of(node).unwrap_or(Role::Peer);
    let (inbound, outbound) = state.degree(node);
    let frame = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.node(node))
        .and_then(|n| n.parent)
        .and_then(|p| state.frames.borrow().get(&p).cloned())
        .unwrap_or_else(|| "unframed".to_owned());
    vec![
        tagged_label(
            "lab.inspector.id",
            name,
            Rect::new(PAD, 44, 180, 20),
            18,
            ink.text,
        ),
        tagged_label(
            "lab.inspector.role",
            format!("{} · frame {frame}", role.name()),
            Rect::new(PAD, 68, 260, 13),
            FONT_SMALL,
            ink.text_3,
        ),
        box_at(
            "lab.inspector.degree",
            Rect::new(PAD, 86, INSP_W - PAD * 2, 24),
            ink.accent_soft,
            Some(ink.accent_line),
            8,
        ),
        tagged_label(
            "lab.inspector.degree.text",
            format!("{inbound} inbound · {outbound} outbound"),
            Rect::new(PAD + 10, 92, 220, 13),
            FONT_SMALL,
            ink.accent,
        ),
    ]
}

/// The pane the identity block and the framework-painted form sit in.
///
/// The two are **siblings**, not nested: the form's geometry is in window
/// coordinates, so putting it inside a pane that is itself absolutely placed
/// would offset it twice — the R1648 defect, and the reason the painter carries
/// its origin.
fn inspector_pane(state: &LabState, theme: &Theme, ink: Ink, mut children: Vec<Scene>) -> Scene {
    let rect = inspector_rect();
    // The form. Everything below this line is the framework's painter — the
    // rows, the type badges, the applies badges, the defects on their rows and
    // the chips that add a key.
    let form = selected_form(state).unwrap_or_default();
    let geometry = inspector_geometry(state);
    let painted = view_config_form("lab.form", &form, &geometry, theme);

    let pending = form.pending_restart();
    let note_y = geometry.origin.1 + geometry.height + 16;
    let restart_note = if pending.is_empty() {
        let hot: Vec<&str> = form
            .fields()
            .iter()
            .filter(|f| f.applies() == Applies::Hot)
            .map(pinion_core::widgets::config_form::ConfigField::key)
            .collect();
        format!("only {} reaches a running node", hot.join(", "))
    } else {
        format!(
            "{} edited; restart to apply",
            pending
                .iter()
                .map(|f| f.key())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let pane = panel("lab.inspector", rect, ink.surface, Some(ink.outline), {
        children.push(box_at(
            "lab.inspector.note",
            Rect::new(PAD, note_y - rect.y, INSP_W - PAD * 2, 40),
            ink.raised,
            Some(ink.warn),
            8,
        ));
        children.push(tagged_label(
            "lab.inspector.note.text",
            restart_note,
            Rect::new(PAD + 10, note_y - rect.y + 8, INSP_W - PAD * 2 - 20, 26),
            10,
            ink.text_2,
        ));
        children
    });

    Scene::Container(
        ContainerNode::new(vec![pane, painted])
            .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
    )
}

fn view(_state: (), _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_lab_state();
    let ink = ink(&theme);
    let win = window_size();

    Scene::Container(
        ContainerNode::new(vec![
            app_bar(&state, ink),
            rail(ink),
            palette(&state, ink),
            toolbar(&state, ink),
            canvas(&state, ink),
            inspector(&state, &theme, ink),
        ])
        .with_tag(VIEW_TAG)
        .with_style(BoxStyle::filled(ink.bg))
        // The root fills the surface the shell gave it, so a resize reflows
        // instead of leaving the rest of the window unpainted.
        .with_layout(LayoutStyle::new().with_size(Size::px(win.0, win.1))),
    )
}

// ── The wire ────────────────────────────────────────────────────────────────

struct LabOracle {
    state: Option<Rc<LabState>>,
}

impl core::fmt::Debug for LabOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LabOracle").finish_non_exhaustive()
    }
}

impl LabOracle {
    const fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: Rc<LabState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<LabState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected("the lab has not been attached"))
    }

    fn text(args: &IntrospectValue) -> Result<String, InvokeError> {
        match args {
            IntrospectValue::Text(s) => Ok(s.clone()),
            other => Err(InvokeError::rejected(format!("{other:?} is not text"))),
        }
    }
}

const FIELDS: &[SchemaField] = &{
    [
        SchemaField::new("spec", "string"),
        SchemaField::new("graph", "string"),
        SchemaField::new("selected", "string"),
        SchemaField::new("selected_link", "string"),
        SchemaField::new("zoom", "int"),
        SchemaField::new("pan", "string"),
        SchemaField::new("running", "bool"),
        SchemaField::new("discovery", "bool"),
        SchemaField::new("cursor", "string"),
        SchemaField::new("verdict", "string"),
        SchemaField::new("gate", "string"),
        SchemaField::new("form", "string"),
        SchemaField::new("document", "string"),
        SchemaField::new("nodes", "string"),
        SchemaField::new("links", "string"),
        SchemaField::new("roles", "string"),
        SchemaField::new("toast", "string"),
        SchemaField::action("select", "string"),
        SchemaField::action("select_link", "string"),
        SchemaField::action_with(
            "set_field",
            "string",
            ArgForm::Delimited('='),
            const {
                &[
                    SchemaArg::key("key", "string", "form"),
                    SchemaArg::open("value", "string"),
                ]
            },
        ),
        SchemaField::action("add_field", "string"),
        SchemaField::action("remove_field", "string"),
        // `zoom_by`, not `zoom`: `zoom` is a declared READ, and one address
        // holding both channels makes "what does this answer" depend on which
        // verb you happened to use. The determinism switch needs no action at
        // all — it is a boolean somebody sets, so it is a WRITE on its read.
        SchemaField::action("zoom_by", "string"),
        SchemaField::action("run", "bool"),
        SchemaField::action_with(
            "connect",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("from", "string", "nodes"),
                    SchemaArg::key("to", "string", "nodes"),
                ]
            },
        ),
        SchemaField::action_with(
            "point",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("x", "int", "cursor"),
                    SchemaArg::key("y", "int", "cursor"),
                ]
            },
        ),
        SchemaField::action("send", "string"),
        SchemaField::action("key", "string"),
    ]
};

impl ExternalIntrospect for LabOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(FIELDS)
    }

    #[allow(clippy::too_many_lines, reason = "one arm per published read")]
    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.state.as_ref()?;
        let text = |s: String| Some(IntrospectValue::Text(s));
        match path {
            "spec" => text(spec_json().to_string()),
            "graph" => text(spec::GRAPH_NAME.to_owned()),
            "selected" => text(state.selected.get().map(|n| state.name_of(n)).unwrap_or_default()),
            "selected_link" => text(
                state
                    .selected_link
                    .get()
                    .map(|l| l.0.to_string())
                    .unwrap_or_default(),
            ),
            "zoom" => Some(IntrospectValue::Int(i64::from(state.zoom.get()))),
            "pan" => {
                let (x, y) = state.pan.get();
                text(format!("{x},{y}"))
            }
            "running" => Some(IntrospectValue::Bool(state.running.get())),
            "discovery" => Some(IntrospectValue::Bool(state.discovery.get())),
            "cursor" => {
                let (x, y) = state.cursor.get();
                text(format!("{x},{y}"))
            }
            "verdict" => {
                let verdict = state.verdict();
                text(
                    serde_json::json!({
                        "may_launch": verdict.may_launch(),
                        "blocking": verdict.blocking(),
                        "warning": verdict.warning(),
                        "sentence": verdict.sentence(),
                    })
                    .to_string(),
                )
            }
            "gate" => text(
                serde_json::Value::Array(
                    state
                        .gate_lines()
                        .into_iter()
                        .map(|(blocks, sentence)| {
                            serde_json::json!({ "blocks": blocks, "sentence": sentence })
                        })
                        .collect(),
                )
                .to_string(),
            ),
            "form" => {
                let form = selected_form(state)?;
                text(
                    serde_json::Value::Array(
                        form.fields()
                            .iter()
                            .map(|f| {
                                serde_json::json!({
                                    "key": f.key(),
                                    "ty": f.ty(),
                                    "applies": f.applies().wire(),
                                    "value": f.value(),
                                    "edited": f.edited(),
                                    "hidden": f.hidden(),
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            "document" => {
                let form = selected_form(state)?;
                match form.document() {
                    Ok(document) => text(document.to_string()),
                    Err(why) => text(serde_json::json!({ "refused": why.to_string() }).to_string()),
                }
            }
            "nodes" => text(
                state
                    .cards()
                    .into_iter()
                    .map(|n| state.name_of(n))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "links" => {
                let doc = state.doc.borrow();
                let tree = doc.tree(ROOT)?;
                text(
                    serde_json::Value::Array(
                        tree.links()
                            .iter()
                            .map(|l| {
                                serde_json::json!({
                                    "id": l.id.0,
                                    "from": state.name_of(l.from.node),
                                    "to": state.name_of(l.to.node),
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            "roles" => text(
                Role::ALL
                    .into_iter()
                    .map(|r| format!("{}:{}", r.group(), r.name()))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "toast" => text(state.toast.get()),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let state = self.state.as_ref().ok_or(InterveneError::UnknownPath)?;
        match path {
            "discovery" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.discovery.set(on);
                Ok(())
            }
            // ★ Refused as READ-ONLY rather than as an unknown path: these
            // three are published reads, and an agent that could write them
            // would be setting a fact the screen derives. Naming the action
            // that does move them is what keeps the refusal useful.
            "running" | "zoom" | "selected" => Err(InterveneError::ReadOnly),

            _ => Err(InterveneError::UnknownPath),
        }
    }

    #[allow(clippy::too_many_lines, reason = "one arm per published action")]
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        match path {
            "select" => {
                let name = Self::text(&args)?;
                let node = state.node_of(name.trim()).ok_or_else(|| {
                    InvokeError::rejected(format!("no node is called {:?}", name.trim()))
                })?;
                state.selected.set(Some(node));
                state.say(format!("selected {}", name.trim()));
                Ok(IntrospectValue::Text(name.trim().to_owned()))
            }
            "select_link" => {
                let raw = Self::text(&args)?;
                let id: u32 = raw
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{raw:?} is not a link id")))?;
                let known = state
                    .doc
                    .borrow()
                    .tree(ROOT)
                    .is_some_and(|t| t.link(LinkId(id)).is_some());
                if !known {
                    return Err(InvokeError::rejected(format!("no link {id} is drawn")));
                }
                state.selected_link.set(Some(LinkId(id)));
                Ok(IntrospectValue::Int(i64::from(id)))
            }
            "set_field" => {
                let raw = Self::text(&args)?;
                let (key, value) = raw.split_once('=').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <key>=<value>"))
                })?;
                let node = state
                    .selected
                    .get()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                let mut forms = state.forms.borrow_mut();
                let form = forms
                    .get_mut(&node)
                    .ok_or_else(|| InvokeError::rejected("the selected node has no form"))?;
                form.set(key.trim(), value.trim())
                    .map_err(|why| InvokeError::rejected(why.to_string()))?;
                let held = form.field(key.trim()).map(|f| f.value().to_owned());
                drop(forms);
                sync_node(&state, node);
                state.say(format!("{} = {}", key.trim(), value.trim()));
                Ok(IntrospectValue::Text(held.unwrap_or_default()))
            }
            "add_field" => {
                let key = Self::text(&args)?;
                let node = state
                    .selected
                    .get()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                let mut forms = state.forms.borrow_mut();
                let form = forms
                    .get_mut(&node)
                    .ok_or_else(|| InvokeError::rejected("the selected node has no form"))?;
                form.add(key.trim())
                    .map_err(|why| InvokeError::rejected(why.to_string()))?;
                drop(forms);
                state.say(format!("added {}", key.trim()));
                Ok(IntrospectValue::Text(key.trim().to_owned()))
            }
            "remove_field" => {
                let key = Self::text(&args)?;
                let node = state
                    .selected
                    .get()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                let mut forms = state.forms.borrow_mut();
                let form = forms
                    .get_mut(&node)
                    .ok_or_else(|| InvokeError::rejected("the selected node has no form"))?;
                form.remove(key.trim())
                    .map_err(|why| InvokeError::rejected(why.to_string()))?;
                drop(forms);
                sync_node(&state, node);
                Ok(IntrospectValue::Text(key.trim().to_owned()))
            }
            "zoom_by" => {
                let word = Self::text(&args)?;
                let now = state.zoom.get();
                let next = match word.trim() {
                    "in" => (now + ZOOM_STEP).min(ZOOM_MAX),
                    "out" => now.saturating_sub(ZOOM_STEP).max(ZOOM_MIN),
                    other => other
                        .parse::<u32>()
                        .ok()
                        .filter(|z| (ZOOM_MIN..=ZOOM_MAX).contains(z))
                        .ok_or_else(|| {
                            InvokeError::rejected(format!(
                                "{other:?} is not `in`, `out`, or {ZOOM_MIN}..={ZOOM_MAX}"
                            ))
                        })?,
                };
                state.zoom.set(next);
                Ok(IntrospectValue::Int(i64::from(next)))
            }
            "run" => {
                let verdict = state.verdict();
                let want = match args {
                    IntrospectValue::Bool(b) => b,
                    _ => !state.running.get(),
                };
                if want && !verdict.may_launch() {
                    return Err(InvokeError::rejected(format!(
                        "the gate is closed: {}",
                        verdict.sentence()
                    )));
                }
                state.running.set(want);
                if want {
                    // A launch settles every form: what is running now IS what
                    // the screen shows, so nothing is pending a restart.
                    for form in state.forms.borrow_mut().values_mut() {
                        form.settle();
                    }
                }
                state.say(if want { "running" } else { "stopped" });
                Ok(IntrospectValue::Bool(want))
            }
            "connect" => {
                let raw = Self::text(&args)?;
                let (from, to) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <from>,<to>")))?;
                let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim()))
                else {
                    return Err(InvokeError::rejected(format!(
                        "{:?} or {:?} is not a node on the canvas",
                        from.trim(),
                        to.trim()
                    )));
                };
                connect(&state, a, b).map(IntrospectValue::Text)
            }
            "point" => {
                let raw = Self::text(&args)?;
                let (x, y) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <x>,<y>")))?;
                let parse = |what: &str, s: &str| -> Result<u32, InvokeError> {
                    s.trim()
                        .parse::<u32>()
                        .map_err(|_| InvokeError::rejected(format!("{what} is a pixel, got {s:?}")))
                };
                let (x, y) = (parse("x", x)?, parse("y", y)?);
                let (win_w, win_h) = window_size();
                if x >= win_w || y >= win_h {
                    return Err(InvokeError::rejected(format!(
                        "({x},{y}) is outside the {win_w}x{win_h} window"
                    )));
                }
                move_cursor(&state, x, y);
                Ok(IntrospectValue::Text(Hit::at(&state, x, y).word(&state)))
            }
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => press(&state),
                    "PointerUp" => release(&state),
                    "PointerLeave" | "PointerCancel" => {
                        state.pressed.borrow_mut().take();
                        state.drag.set(None);
                    }
                    "WheelUp" | "WheelDown" => {
                        let now = state.zoom.get();
                        let next = if event.trim() == "WheelUp" {
                            (now + ZOOM_STEP).min(ZOOM_MAX)
                        } else {
                            now.saturating_sub(ZOOM_STEP).max(ZOOM_MIN)
                        };
                        state.zoom.set(next);
                    }
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerLeave / PointerCancel / WheelUp / WheelDown"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.toast.get()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(key(&state, chord.trim())))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The specification, as the wire publishes it.
fn spec_json() -> serde_json::Value {
    serde_json::json!({
        "panes": spec::PANES.iter().map(|p| serde_json::json!({
            "tag": p.tag, "title": p.title, "width": p.width,
        })).collect::<Vec<_>>(),
        "rail": spec::RAIL.iter().map(|(name, locked)| serde_json::json!({
            "name": name, "locked": locked, "active": *name == spec::RAIL_ACTIVE,
        })).collect::<Vec<_>>(),
        "roles": spec::ROLES.iter().map(|r| serde_json::json!({
            "name": r.name, "gist": r.gist, "group": r.group, "accepts": r.accepts,
        })).collect::<Vec<_>>(),
        "pin_legend": spec::PIN_LEGEND.iter().map(|(k, m)| serde_json::json!({
            "kind": k, "means": m,
        })).collect::<Vec<_>>(),
        "protocols": spec::PROTOCOLS,
        "frames": spec::FRAMES.iter().map(|f| serde_json::json!({
            "name": f.name, "gist": f.gist, "rect": [f.rect.0, f.rect.1, f.rect.2, f.rect.3],
        })).collect::<Vec<_>>(),
        "nodes": spec::NODES.iter().map(|n| serde_json::json!({
            "id": n.id, "role": n.role, "badge": n.badge, "frame": n.frame,
            "rect": [n.rect.0, n.rect.1, n.rect.2],
            "rows": n.rows.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "links": spec::LINKS.iter().map(|(a, b)| serde_json::json!([a, b])).collect::<Vec<_>>(),
        "selected_link": [spec::SELECTED_LINK.0, spec::SELECTED_LINK.1],
        "selected_node": spec::SELECTED_NODE,
        "fields": spec::FIELDS.iter().map(|f| serde_json::json!({
            "key": f.key, "ty": f.ty, "applies": f.applies, "value": f.value,
        })).collect::<Vec<_>>(),
        "addable": spec::ADDABLE,
        "gestures": spec::GESTURES.iter().map(|(g, w)| serde_json::json!([g, w])).collect::<Vec<_>>(),
        "graph": spec::GRAPH_NAME,
        "zoom": spec::OPENING_ZOOM,
    })
}

/// Re-derive what a node's *pins* mean from its form.
///
/// The one place the two halves meet: an endpoint edited in the inspector
/// changes what the canvas draws and what the gate says, and it does so by
/// re-deriving rather than by a second write.
fn sync_node(state: &Rc<LabState>, node: NodeId) {
    let forms = state.forms.borrow();
    let Some(form) = forms.get(&node) else {
        return;
    };
    let listen = form
        .field("listen.endpoints")
        .map_or(String::new(), |f| f.value().to_owned());
    let transport = Transport::of_locator(&listen)
        .or_else(|| {
            form.field("connect.endpoints")
                .and_then(|f| Transport::of_locator(f.value()))
        })
        .unwrap_or(Transport::Tcp);
    drop(forms);
    let mut doc = state.doc.borrow_mut();
    if let Some(slot) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(node)) {
        if let NodeBody::Kind(kind) = &mut slot.body {
            kind.transport = transport;
            kind.listening = !listen.trim().is_empty();
        }
    }
}

/// The accept port on `to` that no link has landed on yet, growing the run by
/// one when they are all taken.
///
/// A dataflow input holds one wire; a listening endpoint is dialled by as many
/// peers as reach it, so the accept pin is a variadic **run** and this is the
/// arithmetic that hides the run from the person authoring the topology. They
/// drag to the pin; which slot they land in is not a decision anybody wants to
/// make.
fn free_accept_port(state: &Rc<LabState>, to: NodeId) -> Option<u32> {
    let taken: Vec<u32> = {
        let doc = state.doc.borrow();
        let tree = doc.tree(ROOT)?;
        tree.links()
            .iter()
            .filter(|l| l.to.node == to)
            .map(|l| l.to.port)
            .collect()
    };
    let arity = {
        let doc = state.doc.borrow();
        u32::try_from(doc.signature(ROOT, to)?.inputs.len()).unwrap_or(0)
    };
    if arity == 0 {
        return None;
    }
    if let Some(free) = (0..arity).find(|p| !taken.contains(p)) {
        return Some(free);
    }
    state
        .doc
        .borrow_mut()
        .insert_item(ROOT, to, Side::Input, arity, Item::plain())
        .ok()?;
    Some(arity)
}

/// Author a link, letting the crate refuse it.
fn connect(state: &Rc<LabState>, from: NodeId, to: NodeId) -> Result<String, InvokeError> {
    let Some(port) = free_accept_port(state, to) else {
        let name = state.name_of(to);
        state.say(format!("{name} has no accept pin"));
        return Err(InvokeError::rejected(format!(
            "{name} does not listen, so nothing can dial it"
        )));
    };
    let made = state
        .doc
        .borrow_mut()
        .connect(ROOT, Socket::new(from, 0), Socket::new(to, port));
    match made {
        Ok(made) => {
            state.selected_link.set(Some(made.link));
            let word = format!("{} -> {}", state.name_of(from), state.name_of(to));
            state.say(format!("linked {word}"));
            Ok(word)
        }
        Err(why) => {
            let sentence = format!("{why:?}");
            state.say(format!("refused: {sentence}"));
            Err(InvokeError::rejected(sentence))
        }
    }
}

fn move_cursor(state: &Rc<LabState>, px: u32, py: u32) {
    state.cursor.set((px, py));
    let Some(drag) = state.drag.get() else {
        return;
    };
    match drag {
        Drag::Pan { from, start } => {
            let dx = i64::from(px) - i64::from(from.0);
            let dy = i64::from(py) - i64::from(from.1);
            state.pan.set((
                start.0 + i32::try_from(dx).unwrap_or(0),
                start.1 + i32::try_from(dy).unwrap_or(0),
            ));
        }
        Drag::Frame { frame, from } => {
            let (ux, uy) = to_canvas(state, px, py);
            let (dx, dy) = (ux - from.0, uy - from.1);
            if dx != 0 || dy != 0 {
                let members = members_of(state, frame);
                let mut doc = state.doc.borrow_mut();
                if let Some(tree) = doc.tree_mut(ROOT) {
                    for id in members.iter().copied().chain(std::iter::once(frame)) {
                        if let Some(slot) = tree.node_mut(id) {
                            slot.x = clamp_to_world(slot.x + dx);
                            slot.y = clamp_to_world(slot.y + dy);
                        }
                    }
                }
                drop(doc);
                state.drag.set(Some(Drag::Frame {
                    frame,
                    from: (ux, uy),
                }));
            }
        }
        Drag::Node { node, grab, snap } => {
            let (ux, uy) = to_canvas(state, px, py);
            let mut cx = ux - grab.0;
            let mut cy = uy - grab.1;
            if snap {
                cx = (cx + SNAP / 2) / SNAP * SNAP;
                cy = (cy + SNAP / 2) / SNAP * SNAP;
            }
            // A node stops at the edge of the world rather than acquiring a
            // position the surface cannot hold.
            cx = clamp_to_world(cx);
            cy = clamp_to_world(cy);
            if let Some(slot) = state
                .doc
                .borrow_mut()
                .tree_mut(ROOT)
                .and_then(|t| t.node_mut(node))
            {
                // ★ R1654 — no second clamp here. `clamp_to_world` above is the
                // bound, and a `.max(0)` beside it silently won: the world's
                // negative half exists so a node can be dragged UP and LEFT of
                // where the graph opened, and pinning the position at zero made
                // the card stop dead partway up the canvas. Two clamps for one
                // fact, and the tighter one decided.
                slot.x = cx;
                slot.y = cy;
            }
        }
        Drag::Wire { .. } => {}
    }
}

fn press(state: &Rc<LabState>) {
    let (px, py) = state.cursor.get();
    let hit = Hit::at(state, px, py);
    match &hit {
        Hit::Node(node) => {
            state.selected.set(Some(*node));
            let (cx, cy) = state
                .doc
                .borrow()
                .tree(ROOT)
                .and_then(|t| t.node(*node))
                .map_or((0, 0), |n| (n.x, n.y));
            let (ux, uy) = to_canvas(state, px, py);
            state.drag.set(Some(Drag::Node {
                node: *node,
                grab: (ux - cx, uy - cy),
                snap: false,
            }));
        }
        Hit::Pin { node, dial: true } => {
            state.drag.set(Some(Drag::Wire { from: *node }));
        }
        Hit::Frame(frame) => {
            state.drag.set(Some(Drag::Frame {
                frame: *frame,
                from: to_canvas(state, px, py),
            }));
        }
        Hit::Canvas => {
            state.drag.set(Some(Drag::Pan {
                from: (px, py),
                start: state.pan.get(),
            }));
        }
        _ => {}
    }
    *state.pressed.borrow_mut() = Some(hit);
}

/// ★ R1654 — a card dropped inside a frame JOINS it, and one dropped outside
/// every frame leaves the one it was in.
///
/// Membership is what a frame's rectangle is derived from, so this is the whole
/// group gesture: the box follows the drop, rather than the drop being checked
/// against a box somebody typed into a table.
fn apply_frame(state: &Rc<LabState>, node: NodeId) {
    let landed = card_rect(state, node)
        .and_then(|r| frame_at(state, i64::from(r.x + r.w / 2), i64::from(r.y + r.h / 2)));
    let held = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.node(node).and_then(|n| n.parent));
    if landed == held {
        return;
    }
    if state
        .doc
        .borrow_mut()
        .set_parent(ROOT, node, landed)
        .is_ok()
    {
        let name = state.name_of(node);
        match landed.and_then(|f| state.frames.borrow().get(&f).cloned()) {
            Some(frame) => state.say(format!("{name} now starts on {frame}")),
            None => state.say(format!("{name} is not on any host")),
        }
    }
}

fn release(state: &Rc<LabState>) {
    let (px, py) = state.cursor.get();
    let now = Hit::at(state, px, py);
    let was = state.pressed.borrow_mut().take();
    let drag = state.drag.get();
    state.drag.set(None);

    // A wire commits on release, onto whatever accept pin it was let go over.
    if let Some(Drag::Wire { from }) = drag {
        if let Hit::Pin { node, dial: false } | Hit::Node(node) = now {
            if node != from {
                connect(state, from, node).ok();
            }
            return;
        }
        state.say("a link needs an accept pin");
        return;
    }
    if let Some(Drag::Node { node, .. }) = drag {
        apply_frame(state, node);
        return;
    }
    if matches!(drag, Some(Drag::Pan { .. } | Drag::Frame { .. })) {
        return;
    }

    let Some(was) = was else { return };
    if was != now {
        return;
    }
    match now {
        Hit::Role(role) => add_node(state, role),
        Hit::DiscoveryToggle => {
            let next = !state.discovery.get();
            state.discovery.set(next);
            state.say(if next {
                "discovery on"
            } else {
                "discovery off"
            });
        }
        Hit::Zoom(up) => {
            let zoom = state.zoom.get();
            state.zoom.set(if up {
                (zoom + ZOOM_STEP).min(ZOOM_MAX)
            } else {
                zoom.saturating_sub(ZOOM_STEP).max(ZOOM_MIN)
            });
        }
        Hit::Run => {
            let verdict = state.verdict();
            if state.running.get() {
                state.running.set(false);
                state.say("stopped");
            } else if verdict.may_launch() {
                state.running.set(true);
                for form in state.forms.borrow_mut().values_mut() {
                    form.settle();
                }
                state.say("running");
            } else {
                state.say(verdict.sentence());
            }
        }
        Hit::Config => {
            let said = selected_form(state).map_or_else(
                || "no node is selected".to_owned(),
                |form| match form.document() {
                    Ok(document) => format!("{} keys", count_leaves(&document)),
                    Err(why) => why.to_string(),
                },
            );
            state.say(said);
        }
        Hit::Link(id) => state.selected_link.set(Some(id)),
        Hit::AddField(key) => {
            if let Some(node) = state.selected.get() {
                let mut forms = state.forms.borrow_mut();
                if let Some(form) = forms.get_mut(&node) {
                    form.add(&key).ok();
                }
                drop(forms);
            }
        }
        Hit::Part { key, part } => act_on_part(state, &key, &part),
        Hit::Rail(name) => state.say(format!("{name} is not this screen")),
        _ => {}
    }
}

fn count_leaves(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(map) => map.values().map(count_leaves).sum(),
        _ => 1,
    }
}

/// Act on an affordance inside a control.
///
/// One dispatcher over the painter's part vocabulary, so the screen answers
/// every shape's affordance rather than the two R1651 drew.
fn act_on_part(state: &Rc<LabState>, key: &str, part: &str) {
    let family = part.split('.').next().unwrap_or_default();
    match family {
        "option" => {
            if let Some(word) = part.rsplit('.').next() {
                toggle_option(state, key, word);
            }
        }
        "toggle" => flip_boolean(state, key),
        "step" => step_number(state, key, part.rsplit('.').next() == Some("up")),
        "item" => {
            if part.rsplit('.').next() == Some("add") {
                add_element(state, key);
            }
        }
        _ => {}
    }
}

/// Flip a boolean field, which is what its checkbox does.
fn flip_boolean(state: &Rc<LabState>, key: &str) {
    let now = state
        .forms
        .borrow()
        .get(&state.selected.get().unwrap_or(NodeId(0)))
        .and_then(|f| f.field(key).map(|v| v.value().trim() == "true"));
    let Some(now) = now else { return };
    set_and_sync(state, key, if now { "false" } else { "true" });
}

/// Move a bounded integer by one, **clamped by the bounds the field declares**.
///
/// The reason a stepper is worth painting: the field knows its range, so the
/// control can refuse to leave it instead of the gate reporting it afterwards.
fn step_number(state: &Rc<LabState>, key: &str, up: bool) {
    let Some(node) = state.selected.get() else {
        return;
    };
    let next = {
        let forms = state.forms.borrow();
        let Some(field) = forms.get(&node).and_then(|f| f.field(key)) else {
            return;
        };
        let FieldType::Integer { min, max } = *field.shape() else {
            return;
        };
        let now: i64 = field.value().trim().parse().unwrap_or(min);
        let step = if up { 1 } else { -1 };
        now.saturating_add(step).clamp(min, max)
    };
    set_and_sync(state, key, next.to_string());
}

/// Append an empty element to a list field, which is what its `+` row does.
fn add_element(state: &Rc<LabState>, key: &str) {
    let Some(node) = state.selected.get() else {
        return;
    };
    let next = {
        let forms = state.forms.borrow();
        let Some(field) = forms.get(&node).and_then(|f| f.field(key)) else {
            return;
        };
        let mut held: Vec<String> = FieldType::elements(field.value())
            .map(str::to_owned)
            .collect();
        held.push(format!("tcp/0.0.0.0:{}", 7400 + held.len()));
        held.join(FieldType::SEPARATOR)
    };
    set_and_sync(state, key, next);
}

/// Write a field and re-derive what the canvas shows from it.
fn set_and_sync(state: &Rc<LabState>, key: &str, value: impl Into<String>) {
    let Some(node) = state.selected.get() else {
        return;
    };
    {
        let mut forms = state.forms.borrow_mut();
        if let Some(form) = forms.get_mut(&node) {
            form.set(key, value).ok();
        }
    }
    sync_node(state, node);
}

/// Turn one option of a choice or flags field on or off.
fn toggle_option(state: &Rc<LabState>, key: &str, word: &str) {
    let Some(node) = state.selected.get() else {
        return;
    };
    let mut forms = state.forms.borrow_mut();
    let Some(form) = forms.get_mut(&node) else {
        return;
    };
    let Some(field) = form.field(key) else { return };
    let one_only = field.shape().one_only();
    let mut chosen: Vec<String> = field
        .value()
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    if one_only {
        chosen = vec![word.to_owned()];
    } else if let Some(at) = chosen.iter().position(|w| w == word) {
        chosen.remove(at);
    } else {
        chosen.push(word.to_owned());
    }
    form.set(key, chosen.join(FieldType::SEPARATOR)).ok();
    drop(forms);
    sync_node(state, node);
}

/// Put a new node of that role at the middle of the canvas.
fn add_node(state: &Rc<LabState>, role: Role) {
    let canvas = canvas_rect();
    let (cx, cy) = to_canvas(state, canvas.x + canvas.w / 2, canvas.y + canvas.h / 2);
    let id = {
        let mut doc = state.doc.borrow_mut();
        doc.add_node(
            ROOT,
            NodeBody::Kind(LabNode {
                role,
                transport: Transport::Tcp,
                listening: false,
            }),
            cx,
            cy,
        )
    };
    let Ok(id) = id else { return };
    let name = format!("{}-{:02}", role.badge(), id.0);
    if let Some(slot) = state
        .doc
        .borrow_mut()
        .tree_mut(ROOT)
        .and_then(|t| t.node_mut(id))
    {
        slot.label = Some(name.clone());
    }
    state.ids.borrow_mut().insert(name.clone(), id);
    state.forms.borrow_mut().insert(id, form_for(&name, role));
    state.selected.set(Some(id));
    state.say(format!("added {name}"));
}

fn key(state: &Rc<LabState>, chord: &str) -> bool {
    match chord {
        "Escape" => {
            state.drag.set(None);
            state.selected_link.set(None);
            true
        }
        "Space" => {
            let verdict = state.verdict();
            if state.running.get() {
                state.running.set(false);
            } else if verdict.may_launch() {
                state.running.set(true);
            } else {
                state.say(verdict.sentence());
                return false;
            }
            true
        }
        "Plus" | "Minus" => {
            let zoom = state.zoom.get();
            state.zoom.set(if chord == "Plus" {
                (zoom + ZOOM_STEP).min(ZOOM_MAX)
            } else {
                zoom.saturating_sub(ZOOM_STEP).max(ZOOM_MIN)
            });
            true
        }
        _ => false,
    }
}

impl External for LabOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// A drag that strays off a pin must keep previewing rather than being
    /// cancelled by a stray pixel.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// The screen tracks the cursor, because a press carries no coordinates.
    fn wants_hover_move(&self) -> bool {
        true
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window fraction times a window size is a pixel inside it"
    )]
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let px = (x_rel.clamp(0.0, 1.0) * window_size().0 as f32) as u32;
        let py = (y_rel.clamp(0.0, 1.0) * WIN_H as f32) as u32;
        move_cursor(&state, px, py);
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

// ── The binding ─────────────────────────────────────────────────────────────

struct NodeLabView;

impl WidgetCore for NodeLabView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = LabOracle::new();
        oracle.attach(use_lab_state());
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
        "pinion hello-node-lab (R1651 §5.21 node graph lab)"
    }
}

impl WidgetA11y for NodeLabView {
    /// The graph is a group, every node is a node, and **the inspector's rows
    /// come from the form painter** — so a control cannot be on screen without
    /// a name, and what its badges say is carried in a status region rather
    /// than lost.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_lab_state();
        let verdict = state.verdict();
        let mut nodes = vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name(format!("{} node graph", spec::GRAPH_NAME))
                .with_value(AccessValue::Text(format!(
                    "{} nodes, {} links, {}",
                    state.cards().len(),
                    state.link_count(),
                    verdict.sentence(),
                ))),
        ];
        for node in state.cards() {
            let name = state.name_of(node);
            let role = state.role_of(node).unwrap_or(Role::Peer);
            let (inbound, outbound) = state.degree(node);
            nodes.push(
                AccessNode::new(format!("lab.node.{name}"), AriaRole::Group)
                    .with_name(name.clone())
                    .with_value(AccessValue::Text(format!(
                        "{}, {inbound} inbound, {outbound} outbound",
                        role.name()
                    ))),
            );
        }
        if let Some(form) = selected_form(&state) {
            let geometry = inspector_geometry(&state);
            nodes.extend(row_access_nodes("lab.form", &form, &geometry));
        }
        nodes
    }
}

impl WidgetView for NodeLabView {
    type Renderer = HelloNodeLabRenderer;

    /// The window OPENS at the design size — the one the specification's
    /// rectangles were measured against — and can be dragged to any size from
    /// there, down to [`MIN_W`] x [`MIN_H`].
    ///
    /// ★ R1654 — it was `Fixed`, which pins the OS-resize FLOOR at the open
    /// size: the window could be enlarged and never shrunk. Together with the
    /// pane rectangles being constants, that made the screen the one size it
    /// was written at. Both halves had to move — a layout that follows the
    /// window is no use if the window cannot be resized.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((MIN_W, MIN_H)),
        }
    }
}

fn main() {
    pinion_shell::run::<NodeLabView>();
}

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;
