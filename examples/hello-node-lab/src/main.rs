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
use pinion_core::availability::Unavailable;
use pinion_core::containment::line_box;
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Signal, Tracked};
use pinion_core::scene::{
    ContainerNode, PathCommand, PathNode, PathPoint, Rect, ScrollAxis, ScrollNode, TextNode,
};
use pinion_core::style::{
    Border, BoxStyle, Color, Dash, LayoutStyle, PathStyle, Size, Stroke, TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::config_form::{
    Applies, ConfigDefect, ConfigField, ConfigForm, FieldType, Verdict,
};
use pinion_core::widgets::scroll::{AutoScroll, ScrollState};
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{CellKind, Frame, Modifiers, Scene, WidgetCore, edit_field_keymap};
use pinion_node_graph::{
    Document, Item, LinkId, LinkLayer, Node, NodeBody, NodeId, ROOT, Relinked, Side, Socket,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::config_form::{
    FieldGrowth, FormGeometry, FormStyle, RowWrap, form_geometry, row_access_nodes,
    view_config_form,
};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::text_field as tf_paint;

use graph::{LabNode, Role, Transport};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloNodeLabRenderer, HelloNodeLabRendererError);

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = 1440;
const WIN_H: u32 = 900;
const VIEW_TAG: &str = "node_lab";
/// R1662 — the input-router tags the two scrolling side panes answer to,
/// **taken from the specification** rather than spelled a second time here. The
/// gate reads the same column, so a pane whose body tag is edited in one place
/// cannot pass by being edited in two.
const PALETTE_SCROLL: &str = match spec::PANES[1].body {
    Some(tag) => tag,
    None => panic!("the palette declares a scrolling body"),
};
const INSPECTOR_SCROLL: &str = match spec::PANES[3].body {
    Some(tag) => tag,
    None => panic!("the inspector declares a scrolling body"),
};
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
/// ★ R1656 — the width the toolbar's RIGHT-anchored cluster needs. The zoom
/// pair, the readout, the config button and the run button are all placed by
/// subtracting a constant from the pane's right edge, so a pane narrower than
/// this paints them off its own left side — and the floor below is what
/// declares that width supported.
const TOOLBAR_RIGHT_CLUSTER: u32 = 300;

/// ★ R1656 — the canvas pane's floor is DERIVED from what the chrome above it
/// needs, not asserted at 240. The size axis found the difference on its first
/// run: at the old floor the zoom readout was not painted at all, because
/// `right - 300` had gone past the pane's own left edge. A declared minimum the
/// screen cannot actually paint is a claim nobody was checking.
const MIN_W: u32 = RAIL_W + PALETTE_W + (TOOLBAR_RIGHT_CLUSTER + TOOLBAR_LEFT_CLUSTER) + INSP_W;

/// ★ R1656 — the toolbar's LEFT half: the graph title, the node/link counts and
/// the launch-gate chip, which is placed after them. Named for the same reason
/// its sibling is — the floor has to be wide enough for both halves, and the
/// size axis found the gate chip painted past the pane's right edge when it
/// was not.
const TOOLBAR_LEFT_CLUSTER: u32 = 420;
/// What the canvas needs vertically once the side panes stop dictating the
/// floor: the launch-gate panel is anchored to the canvas bottom and the hint
/// strip sits under it, so a canvas shorter than the two together paints one
/// over the other.
const CANVAS_FLOOR: u32 = 260;

/// The smallest height, likewise.
///
/// ★ R1662 — R1656 wrote the answer down and could not take it: "the panes do
/// not scroll, so the floor IS their content height; making them scroll would
/// let this number come back down and is the better answer". They scroll now,
/// so it came down — from 680 to what the CANVAS chrome needs, which is the
/// same derivation the width already used. A floor set by content nobody could
/// scroll to is a window a person cannot make small, and it was 420 pixels of
/// it ([[debt-the-node-lab-panes-do-not-scroll]]).
const MIN_H: u32 = APP_BAR_H + TOOLBAR_H + CANVAS_FLOOR;

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
    /// ★ R1681 — a link that is already there is being re-aimed: it was picked
    /// up off the accept pin it lands on, and it follows the cursor from the
    /// pin that dials it.
    ///
    /// The link stays in the document for the whole drag, which the reference
    /// cannot do — its author-a-link has no way to move an end, so it splices
    /// the wire out on pick-up and re-adds it on drop, and a release that
    /// refuses has to remember to put it back. Here the move is one atomic
    /// verb, so nothing is taken out until something takes its place.
    Rewire { link: LinkId, from: NodeId },
    /// A host frame is being moved, and every card it holds moves with it.
    Frame { frame: NodeId, from: (i32, i32) },
}

/// Which link the canvas has picked out (R1681).
///
/// Two arms because there are two kinds of link on this canvas and only one of
/// them is in the graph. A reported link is a **claim about** the topology, it
/// carries no [`LinkId`] because it is not a link, and the affordance it offers
/// is the opposite one: an authored link can be deleted, and a reported link
/// can only be taken into the drawing. The reference reaches the same split and
/// spells it as a flag on the wire plus a predicate; the difference is that
/// here a reported link cannot be handed to anything that takes a `LinkId`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum LinkPick {
    /// One somebody drew.
    Authored(LinkId),
    /// One a source reported, named by the pair it runs between — which is the
    /// only identity an observation has.
    Observed(Socket, Socket),
}

impl LinkPick {
    /// The authored link, or `None` for a reported one.
    const fn authored(self) -> Option<LinkId> {
        match self {
            Self::Authored(id) => Some(id),
            Self::Observed(..) => None,
        }
    }
}

/// ★★★ R1683 — what the screen's ONE text field is editing, or `None` while it
/// is closed.
///
/// One field with a target rather than a field per site, which is the sibling
/// node editor's arrangement and the reason it is worth copying: a second field
/// is a second focus owner, a second keymap and a second commit path, and the
/// three sites here want exactly the same behaviour over different values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum Editing {
    /// The selected card's name.
    Name(NodeId),
    /// A configuration path being typed into the selected card's form, which
    /// the catalogue does not offer.
    Key(NodeId),
}

impl Editing {
    /// The word the wire reads this back as.
    const fn wire(self) -> &'static str {
        match self {
            Self::Name(_) => "name",
            Self::Key(_) => "key",
        }
    }

    /// What the keystroke gate lets through. Both targets are free text — a
    /// node's name and a configuration path — so this is `Text` for now and is
    /// a `match` rather than a constant because the next target may not be.
    const fn kind(self) -> CellKind {
        match self {
            Self::Name(_) | Self::Key(_) => CellKind::Text,
        }
    }
}

/// The tag the shared field is painted under, which is also what owns focus
/// while it is open.
const EDIT_TAG: &str = "lab.edit";

/// Everything the screen is.
struct LabState {
    doc: Tracked<Document<LabNode>>,
    forms: Tracked<BTreeMap<NodeId, ConfigForm>>,
    frames: RefCell<BTreeMap<NodeId, String>>,
    /// ★★ R1679 — where each card came into being: its canvas position and the
    /// host it started on.
    ///
    /// A record rather than a derivation, and the reason is the one case a
    /// derivation cannot cover. The opening cards' placement IS in
    /// [`spec::NODES`] and R1678 compared against it directly — but a card the
    /// palette adds is not in the specification at all, so it had no baseline,
    /// and the layout predicate answered "unchanged" for a card a person had
    /// visibly dragged across the canvas.
    ///
    /// Written once at creation, from the specification for the opening cards
    /// and from the placement arithmetic for an added one, so the two kinds of
    /// card answer the same question the same way and the scope has ONE
    /// population instead of a rule with an exception in it.
    opened_at: RefCell<BTreeMap<NodeId, Placement>>,
    selected: Signal<Option<NodeId>>,
    selected_link: Signal<Option<LinkPick>>,
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
    /// ★★ R1683 — what the shared field is editing, or `None` while it is shut.
    editing: Signal<Option<Editing>>,
    /// The buffer that field holds.
    ///
    /// ★★★ **The hook's own object, taken once when the screen is built — not
    /// a second one.** `use_text_edit_state` resolves through the shell's root
    /// owner, which lives as long as the application, so the painter and this
    /// hold ONE buffer; but it PANICS outside an owner scope, and this screen's
    /// pointer handlers and its wire both run outside one (the same fact R1662
    /// met with the scroll offsets). So the reference is taken where an owner
    /// is guaranteed — here, since this type is only ever constructed from
    /// inside one — and kept for the paths that have none.
    ///
    /// ★ Taken at construction rather than at the first paint, and the gate is
    /// what forced it: an agent that opened the editor before the screen had
    /// painted got "there is no field", which is a state no session reaches and
    /// a refusal nobody should have to think about.
    ///
    /// That the two are one object is asserted rather than assumed —
    /// `r1683_the_screen_and_the_painter_hold_one_buffer` compares them with
    /// `Rc::ptr_eq` after a paint, which is what would fail if the owner were
    /// ever per-frame.
    buffer: Rc<TextEditState>,
    /// R1662 — the two side panes' scroll offsets, held here rather than
    /// reached for with `use_scroll_state` because the paint and the hit test
    /// both need them and only the paint runs inside an `Owner` scope. One
    /// object, so the two cannot read two facts.
    palette_scroll: Rc<ScrollState>,
    inspector_scroll: Rc<ScrollState>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<LabState>>> = const { RefCell::new(None) };
}

/// Put the screen back to the state it opens in.
///
/// ★ R1677 — the operation gate needs it, and needs it for a reason worth
/// stating: it asks of each operation "does causing it change something", which
/// is only a fair question from a screen that has not already been changed by
/// the operation before. The swept states next door are deliberately
/// cumulative — a session with the tool is one edit on top of another — and
/// that is the wrong shape for a gate whose rows have to be independent.
///
/// Test-only because production has exactly one screen and never wants a
/// second: a reset reachable from the running application would be an
/// operation nobody declared.
#[cfg(test)]
fn reset_lab_state() {
    STATE.with(|slot| *slot.borrow_mut() = None);
}

/// The scroll state a pane body tag names, or `None` for a tag that is not a
/// pane body.
///
/// ★ R1662 — one lookup, so a caller that has a `scene/scroll_reach` row (which
/// names the viewport by tag) can act on it without a second table mapping tags
/// to states. Test-only here because the screen's own two call sites reach
/// their state by name; the wire drives a pane through `scene/scroll`, which
/// resolves the node's own [`ScrollState`] and never needs this table.
/// A press, moved into the frame a scrolling pane's rectangles are stated in.
///
/// ★ R1662 — the pane's content slides under a fixed viewport, so the paint of
/// a row is `row - offset` and the only way for a hit test written against
/// `row` to stay true is to ask about `point + offset`. One direction, one
/// place: the alternative is a second set of rectangles that has to be kept in
/// step, which is what [`FormGeometry::translated`] avoids on the other pane —
/// there the geometry itself is published to assistive technology, so it is the
/// geometry that moves.
fn in_pane(scroll: &ScrollState, px: u32, py: u32) -> (u32, u32) {
    let (ox, oy) = scroll.offset();
    let fold = |v: u32, by: i32| -> u32 {
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "clamped into u32's range on the line above the cast"
        )]
        let folded = (i64::from(v) + i64::from(by)).clamp(0, i64::from(u32::MAX)) as u32;
        folded
    };
    (fold(px, ox), fold(py, oy))
}

#[cfg(test)]
fn pane_scroll<'s>(state: &'s LabState, body: &str) -> Option<&'s Rc<ScrollState>> {
    match body {
        PALETTE_SCROLL => Some(&state.palette_scroll),
        INSPECTOR_SCROLL => Some(&state.inspector_scroll),
        _ => None,
    }
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

        let selected_link = seed_links(&mut doc, &forms, &ids).map(LinkPick::Authored);

        let selected = ids.get(spec::SELECTED_NODE).copied();
        // R1679 — the opening placement of every card the specification
        // describes, recorded from the specification itself so the record and
        // the graph cannot have been built from different numbers.
        let opened_at: BTreeMap<NodeId, Placement> = spec::NODES
            .iter()
            .filter_map(|want| {
                let id = *ids.get(want.id)?;
                let (x, y, _) = want.rect;
                Some((
                    id,
                    Placement {
                        at: (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
                        host: Some(want.frame.to_owned()),
                        opened_as: Some(want.id),
                    },
                ))
            })
            .collect();
        // ★ R1682 — `ids` above is a BUILD-TIME convenience and is dropped
        // here. It used to be kept as a field, which made it a second record of
        // what a card is called; the document holds the one record now, and a
        // map that dies at the end of this function cannot drift from anything.
        Self {
            doc: Tracked::new(doc),
            forms: Tracked::new(forms),
            frames: RefCell::new(frames),
            opened_at: RefCell::new(opened_at),
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
            editing: Signal::new(None),
            buffer: use_text_edit_state(EDIT_TAG),
            palette_scroll: Rc::new(ScrollState::with_tag(PALETTE_SCROLL)),
            inspector_scroll: Rc::new(ScrollState::with_tag(INSPECTOR_SCROLL)),
        }
    }

    fn say(&self, what: impl Into<String>) {
        self.toast.set(what.into());
    }

    /// The node the canvas labels `id`, or `None`.
    ///
    /// ★★ R1682 — the DOCUMENT's answer. This screen kept its own
    /// `BTreeMap<String, NodeId>` beside the document's own
    /// [`Node::label`](pinion_node_graph::Node::label) until the rename arrived
    /// and made the duplication load-bearing: two records of one fact, and a
    /// rename that updated either one alone would leave the canvas and the wire
    /// calling the same card two different things. The model owns names now —
    /// it is the thing that can *refuse* a name already taken — so there is one
    /// record and no way to update half of it.
    fn node_of(&self, id: &str) -> Option<NodeId> {
        self.doc.borrow().node_labelled(ROOT, id)
    }

    fn name_of(&self, node: NodeId) -> String {
        self.doc
            .borrow()
            .tree(ROOT)
            .and_then(|tree| tree.node(node))
            .map_or_else(|| format!("#{}", node.0), Node::display_name)
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

/// ★★★ R1678 — what a reset puts back, and the fact its affordance is derived
/// from.
///
/// The reference tool keeps every edit as an OVERLAY on the opening state, so
/// each of its five resets is one `clear` and each "is there anything to put
/// back" is one `is_empty`. This screen mutates its document in place instead,
/// so both facts are derived by comparing against [`crate::spec`] — which is
/// where the opening state came from in the first place (`seed_nodes` /
/// `seed_links` build it from exactly these constants), so there is no snapshot
/// that could fall out of step with anything.
///
/// **The two halves are one type on purpose.** A screen that decided for itself
/// when to show a "put it back" affordance would be a second author of the
/// rule, and the failure mode is silent in both directions: an affordance shown
/// over an unchanged screen does nothing when pressed, and one hidden over a
/// changed screen strands the change. [`changed`](Self::changed) and
/// [`apply`](Self::apply) are asserted against each other — after an apply, the
/// scope reports unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResetScope {
    /// Which cards exist — the palette's additions go away.
    Nodes,
    /// Where the cards sit AND which host each starts on.
    ///
    /// One scope covering two facts because that is what the reference does
    /// (measured: its layout reset clears both maps and says so in its own
    /// toast). They belong together — a card dragged onto another host has
    /// moved and been re-parented by one gesture, so putting one back without
    /// the other leaves a state no gesture could have produced.
    Layout,
    /// Every form: values, and rows added or taken away.
    Fields,
    /// The authored links, and which one is selected.
    Links,
    /// Pan and zoom.
    View,
}

impl ResetScope {
    /// The census. Consumers iterate this rather than re-listing the arms.
    const ALL: [Self; 5] = [
        Self::Nodes,
        Self::Layout,
        Self::Fields,
        Self::Links,
        Self::View,
    ];

    /// The scope words, as the declaration publishes them.
    ///
    /// Built FROM [`ALL`](Self::ALL) rather than listed beside it, so the
    /// vocabulary an agent is offered and the arms this type has cannot come
    /// apart — a hand-written copy would still compile with an arm missing.
    const WIRE_NAMES: [&'static str; Self::ALL.len()] = {
        let mut out = [""; Self::ALL.len()];
        let mut n = 0;
        while n < Self::ALL.len() {
            out[n] = Self::ALL[n].wire();
            n += 1;
        }
        out
    };

    /// The word the wire and the specification call this scope.
    const fn wire(self) -> &'static str {
        match self {
            Self::Nodes => "nodes",
            Self::Layout => "layout",
            Self::Fields => "fields",
            Self::Links => "links",
            Self::View => "view",
        }
    }

    /// Whether this scope has an affordance ON THE PANEL, which it has only
    /// when there is something to put back.
    ///
    /// ★ The view is deliberately not one of these — measured on the reference,
    /// its four graph resets are wrapped in a conditional and its VIEW reset is
    /// not, sitting unconditionally in the zoom cluster. That asymmetry is a
    /// judgement worth keeping: pan and zoom always have a home to go to and
    /// the button is one glyph wide, while a graph reset that appears out of
    /// nowhere over an untouched screen is an invitation to destroy work.
    const fn gated(self) -> bool {
        !matches!(self, Self::View)
    }

    /// Whether the screen differs from what it opened as, in this scope.
    fn changed(self, state: &LabState) -> bool {
        match self {
            // ★★ R1682 — by IDENTITY, not by re-deriving the opening set from
            // what the cards are currently called. A card is a stray when
            // nothing recorded it opening; an opening card differs when it no
            // longer shows the name it opened as. Comparing the name list
            // against the specification answered both questions with one
            // string comparison, and a rename makes those two questions give
            // opposite answers about the same card.
            Self::Nodes => {
                let cards = state.cards();
                let opened = state.opened_at.borrow();
                cards.len() != spec::NODES.len()
                    || cards.iter().any(|node| {
                        opened.get(node).and_then(|born| born.opened_as)
                            != Some(state.name_of(*node)).as_deref()
                    })
            }
            // ★ R1679 — over EVERY card, against where each came into being.
            // The population was `spec::NODES`, which cannot see a card the
            // palette added: measured, dragging one moved it 60 by 36 and this
            // answered false.
            Self::Layout => state
                .cards()
                .into_iter()
                .any(|node| placed_as_opened(state, node) == Some(false)),
            // The form answers for itself — values and shape both. See
            // `ConfigForm::edited`, which is where that question belongs.
            Self::Fields => state.forms.borrow().values().any(ConfigForm::edited),
            Self::Links => {
                let doc = state.doc.borrow();
                let Some(tree) = doc.tree(ROOT) else {
                    return false;
                };
                let mut now: Vec<(String, String)> = tree
                    .links()
                    .iter()
                    .map(|l| (state.name_of(l.from.node), state.name_of(l.to.node)))
                    .collect();
                let mut want: Vec<(String, String)> = spec::LINKS
                    .iter()
                    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                    .collect();
                now.sort();
                want.sort();
                now != want
            }
            Self::View => state.zoom.get() != spec::OPENING_ZOOM || state.pan.get() != (0, 0),
        }
    }

    /// Put this scope back to what the screen opened with.
    fn apply(self, state: &Rc<LabState>) {
        match self {
            Self::Nodes => put_node_set_back(state),
            Self::Layout => put_cards_back(state),
            Self::Fields => {
                let nodes: Vec<NodeId> = state.forms.borrow().keys().copied().collect();
                for form in state.forms.borrow_mut().values_mut() {
                    form.revert();
                }
                // The pins are DERIVED from the form (`sync_node`), so a revert
                // that stopped at the values would leave a card drawing the
                // transport of an endpoint it no longer holds.
                for node in nodes {
                    sync_node(state, node);
                }
            }
            // ★★ R1679 — a DIFF, not a rebuild, and the gate is what forced it.
            //
            // The first version cleared every link and re-authored the whole
            // set from the specification. Correct in what it left behind and
            // wrong in what it published: the model assigns a fresh identifier
            // to each new link, so putting an UNTOUCHED graph back renumbered
            // all seven of them. `r1679_a_reset_affordance_is_painted_exactly_
            // when_it_would_do_something` caught it in all eight swept states —
            // the affordance was correctly absent and pressing it would still
            // have changed the screen.
            //
            // The reference's reset is idempotent because it drops an overlay
            // that is already empty. This one has to earn that: it removes only
            // the links the specification does not have and adds only the ones
            // it is missing, so a link nobody touched keeps its identity and a
            // reset over an unchanged graph does nothing at all.
            //
            // ★ Deliberately does NOT consult `changed`. An `apply` that
            // early-returned on "nothing differs" would make the gate above
            // compare the predicate with itself, which is the tautology this
            // whole round exists to remove.
            Self::Links => {
                let mut want: Vec<(String, String)> = spec::LINKS
                    .iter()
                    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                    .collect();
                let mut drop_these: Vec<LinkId> = Vec::new();
                {
                    let doc = state.doc.borrow();
                    if let Some(tree) = doc.tree(ROOT) {
                        for link in tree.links() {
                            let pair = (state.name_of(link.from.node), state.name_of(link.to.node));
                            // One `want` entry per live link, so a duplicated
                            // pair keeps exactly as many as the specification
                            // declares and no more.
                            match want.iter().position(|p| *p == pair) {
                                Some(at) => {
                                    want.remove(at);
                                }
                                None => drop_these.push(link.id),
                            }
                        }
                    }
                }
                {
                    let mut doc = state.doc.borrow_mut();
                    for link in drop_these {
                        doc.disconnect(ROOT, link).ok();
                    }
                }
                for (from, to) in want {
                    let (Some(a), Some(b)) = (state.node_of(&from), state.node_of(&to)) else {
                        continue;
                    };
                    connect(state, a, b).ok();
                }
                if let Some(id) = state
                    .node_of(spec::SELECTED_LINK.0)
                    .zip(state.node_of(spec::SELECTED_LINK.1))
                    .and_then(|(a, b)| {
                        let doc = state.doc.borrow();
                        let tree = doc.tree(ROOT)?;
                        tree.links()
                            .iter()
                            .find(|l| l.from.node == a && l.to.node == b)
                            .map(|l| l.id)
                    })
                {
                    state.selected_link.set(Some(LinkPick::Authored(id)));
                }
            }
            Self::View => {
                state.zoom.set(spec::OPENING_ZOOM);
                state.pan.set((0, 0));
            }
        }
    }
}

/// Put the card SET back: the palette's additions go, and every opening card
/// answers to the name it opened as.
///
/// ★★ R1682 — the two halves are one operation and the second is what renaming
/// forced. A stray is a card with **no opening record**; a renamed opening card
/// is not one. Selecting strays by "its name is not in the specification" — the
/// only question there was to ask before names could change — deleted the very
/// card whose name this scope exists to put back.
fn put_node_set_back(state: &Rc<LabState>) {
    let strays: Vec<NodeId> = state
        .cards()
        .into_iter()
        .filter(|n| {
            state
                .opened_at
                .borrow()
                .get(n)
                .and_then(|born| born.opened_as)
                .is_none()
        })
        .collect();
    // Which names have to go back, decided BEFORE the strays are removed: a
    // name freed by a deletion is one a rename may have taken.
    let restore: Vec<(NodeId, &'static str)> = state
        .cards()
        .into_iter()
        .filter(|n| !strays.contains(n))
        .filter_map(|n| {
            let born = state.opened_at.borrow().get(&n)?.opened_as?;
            (state.name_of(n) != born).then_some((n, born))
        })
        .collect();
    {
        let mut doc = state.doc.borrow_mut();
        for node in &strays {
            doc.remove_node(ROOT, *node).ok();
        }
    }
    state
        .forms
        .borrow_mut()
        .retain(|id, _| !strays.contains(id));
    // ★ R1679 close-audit — and its placement with it. Every other per-card map
    // is cleaned here; `opened_at` was added that session and missed, which
    // would leave a placement behind for a card that no longer exists.
    // Harmless today because the model does not reuse an identifier, and
    // exactly the kind of "harmless today" that stops being so without a diff.
    state
        .opened_at
        .borrow_mut()
        .retain(|id, _| !strays.contains(id));
    for (node, name) in restore {
        // Through the same verb the rename action uses, so "put the name back"
        // and "change the name" cannot be two rules about what a name is. It
        // cannot be refused here: the name is the one this card opened with,
        // and whatever had taken it was either renamed away or is a stray now
        // gone.
        rename_card(state, node, name).ok();
    }
    if state.selected.get().is_some_and(|n| strays.contains(&n)) {
        state.selected.set(state.node_of(spec::SELECTED_NODE));
    }
}

/// Put every card back where it came into being, on the host it started on.
///
/// One function because the two halves are one operation — see [`Placement`].
fn put_cards_back(state: &Rc<LabState>) {
    for node in state.cards() {
        let Some(opened) = state.opened_at.borrow().get(&node).cloned() else {
            continue;
        };
        let frame = opened.host.and_then(|want| {
            state
                .frames
                .borrow()
                .iter()
                .find(|(_, name)| **name == want)
                .map(|(id, _)| *id)
        });
        let mut doc = state.doc.borrow_mut();
        if let Some(slot) = doc.tree_mut(ROOT).and_then(|t| t.node_mut(node)) {
            slot.x = opened.at.0;
            slot.y = opened.at.1;
        }
        doc.set_parent(ROOT, node, frame).ok();
    }
}

/// Where a card came into being: its canvas position, and the host it started
/// on — `None` for a card the palette added, which belongs to no host until it
/// is dropped on one.
///
/// A named pair rather than a tuple because both halves are put back TOGETHER
/// by one operation (the reference's layout reset clears position and host in
/// one call and says so in its own message), and a caller holding two loose
/// values can restore one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Placement {
    at: (i32, i32),
    host: Option<String>,
    /// ★★ R1682 — the name in [`spec::NODES`] this card came into being as, or
    /// `None` for one the palette added.
    ///
    /// A third field on the same record rather than a second map keyed the same
    /// way, for the R1679 reason this record exists at all: it is written ONCE,
    /// where the card is created, so the two kinds of card answer the same
    /// question the same way. It is a *different* scope's business — the node
    /// reset puts names back, the layout reset puts the pair above back — and
    /// [`placed_as_opened`] deliberately does not look at it.
    ///
    /// **Renaming is what forced it.** Before it, the node reset told an
    /// opening card from an added one by comparing its NAME against the
    /// specification, which is exactly the thing a rename changes: a renamed
    /// opening card read as a stray, and the reset that was supposed to put its
    /// name back would have deleted the node instead. The same lesson R1679
    /// wrote for the link reset — put back by identity, never by re-deriving
    /// from what a thing is currently called.
    opened_as: Option<&'static str>,
}

/// Whether this card sits where it came into being, on the host it started on.
///
/// `None` when nothing recorded its opening placement — which cannot happen for
/// a card this screen created, and is answered as "no opinion" rather than as
/// "unchanged" so a gap in the record can never read as a clean screen.
fn placed_as_opened(state: &LabState, node: NodeId) -> Option<bool> {
    let opened = state.opened_at.borrow().get(&node).cloned()?;
    let doc = state.doc.borrow();
    let slot = doc.tree(ROOT).and_then(|t| t.node(node))?;
    let host = slot
        .parent
        .and_then(|f| state.frames.borrow().get(&f).cloned());
    Some((slot.x, slot.y) == opened.at && host == opened.host)
}

/// The scopes with something to put back, in census order — the panel's
/// affordances, and the one list both the paint and the hit test read.
fn changed_scopes(state: &LabState) -> Vec<ResetScope> {
    ResetScope::ALL
        .into_iter()
        .filter(|scope| scope.gated() && scope.changed(state))
        .collect()
}

/// Author the opening links onto `doc`, and answer which one the screen opens
/// with selected.
///
/// ★ R1678 — lifted out of `opening` because a reset PUTS THESE BACK, and the
/// port-picking below is the kind of arithmetic that is quietly wrong in a
/// second copy: an accept pin is a variadic run, so which slot a link lands in
/// depends on what is already there. Two implementations would agree on the
/// opening graph (nothing is there yet) and disagree the moment a reset ran
/// over a graph somebody had edited — which is exactly when nobody is looking.
fn seed_links(
    doc: &mut Document<LabNode>,
    forms: &BTreeMap<NodeId, ConfigForm>,
    ids: &BTreeMap<String, NodeId>,
) -> Option<LinkId> {
    let mut selected_link = None;
    for (from, to) in spec::LINKS {
        let (Some(&a), Some(&b)) = (ids.get(*from), ids.get(*to)) else {
            continue;
        };
        // ★ R1681 — the SAME endpoint arithmetic the canvas uses, not a second
        // copy of it. Port 0 on the dial side: the taxonomy declares one.
        let Ok(endpoint) = landing_endpoint(doc, forms, a, b) else {
            continue;
        };
        let Some(port) = open_slot_in(doc, b, endpoint.as_deref()) else {
            continue;
        };
        match doc.connect(ROOT, Socket::new(a, 0), Socket::new(b, port)) {
            Ok(made) => {
                if (*from, *to) == spec::SELECTED_LINK {
                    selected_link = Some(made.link);
                }
            }
            Err(_) => {
                doc.remove_item(ROOT, b, Side::Input, port).ok();
            }
        }
    }
    // ★ R1681 — what a source REPORTED, beside what the specification drew.
    // Seeded here because it is part of the opening state a reset puts back,
    // and because an "adopt" affordance with nothing to adopt is an affordance
    // no test and no person can reach.
    for (from, to) in spec::OBSERVED {
        let (Some(&a), Some(&b)) = (ids.get(*from), ids.get(*to)) else {
            continue;
        };
        let Ok(endpoint) = landing_endpoint(doc, forms, a, b) else {
            continue;
        };
        let Some(port) = open_slot_in(doc, b, endpoint.as_deref()) else {
            continue;
        };
        if doc
            .observe(ROOT, Socket::new(a, 0), Socket::new(b, port))
            .is_err()
        {
            doc.remove_item(ROOT, b, Side::Input, port).ok();
        }
    }
    selected_link
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

/// The specification row this card came into being as, or `None` for one the
/// palette added.
///
/// ★★ R1682 — by the name the card OPENED with, never by what it is called
/// now. Two derivations keyed off the current name — the card's digest rows and
/// its width — and a rename silently changed both: a renamed card stopped
/// matching any specification row, so it fell through to the palette-added
/// path, redrew its digest from the first three form fields and snapped to the
/// default width. Nothing was broken enough to fail, which is how it would have
/// stayed.
fn declared_card(state: &LabState, node: NodeId) -> Option<&'static spec::NodeSpec> {
    let opened_as = state.opened_at.borrow().get(&node)?.opened_as?;
    spec::NODES.iter().find(|n| n.id == opened_as)
}

/// Whether this card is drawn small.
fn card_collapsed(state: &LabState, node: NodeId) -> bool {
    state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .is_some_and(|slot| slot.appearance.collapsed)
}

/// The digest lines a node's card shows.
///
/// The declared ones for a node the specification opens with, and otherwise the
/// first three rows of the node's own form — so a node added from the palette
/// is a card like any other rather than an empty box.
fn card_rows(state: &LabState, node: NodeId) -> Vec<(String, String)> {
    if let Some(declared) = declared_card(state, node) {
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

/// How wide a collapsed card is drawn, in canvas units.
///
/// Narrower than any card the specification declares, so collapsing is visible
/// at a glance rather than only by counting rows. The reference collapses to a
/// fixed width for the same reason, and everything that follows the card — its
/// pins, the wires into them, the frame that bounds it — is derived from this
/// width and follows without being told.
const CARD_COLLAPSED_W: u32 = 92;

/// The width a node's card is drawn at, in canvas units.
fn card_width(state: &LabState, node: NodeId) -> u32 {
    if card_collapsed(state, node) {
        return CARD_COLLAPSED_W;
    }
    declared_card(state, node).map_or(146, |declared| declared.rect.2)
}

/// Every rectangle a node's card is made of, derived once.
///
/// ★ R1656 — the card's own box is computed FROM the rows it paints, and the
/// rows are placed in the same pass. Before this they were two derivations of
/// one fact: the height was `scaled(HDR + rows * ROW_H + 6)` while the rows
/// were placed at `y + HDR + n * ROW_H` **unscaled**, so at any zoom below 100%
/// the last row was painted below the border — measured at the size the screen
/// opens in, seven of eight cards spilled by three to five pixels, and a person
/// reported it before any check here did.
///
/// Written as a shape rather than as a rule ("remember to scale both") for the
/// reason [`pinion_core::widgets::config_form`]'s row parts are: a rule can be
/// half-applied and a derivation cannot. `rect.h` is the union of the parts,
/// so a row that does not fit is not expressible.
struct CardShape {
    /// The card's box, in window pixels.
    rect: Rect,
    /// The identity label, relative to `rect`.
    id: Rect,
    /// The role badge and the text inside it, relative to `rect`.
    badge: Rect,
    /// The badge's label, relative to `rect`.
    badge_text: Rect,
    /// The face the identity label is drawn at — scaled, so it shrinks with
    /// the diagram it belongs to.
    id_font: u32,
    /// The face a digest row is drawn at.
    row_font: u32,
    /// The face the role badge is drawn at.
    badge_font: u32,
    /// One (key, value) pair of rectangles per digest row, relative to `rect`.
    rows: Vec<(Rect, Rect)>,
}

/// The size a face on the canvas is drawn at: it scales with the zoom, because
/// a node card is part of the diagram and not chrome over it.
///
/// ★ R1656 — this scaling did not exist. The card's BOX was scaled while its
/// font and its row pitch were not, so at any zoom below 100% the two disagreed
/// and the disagreement was painted: rows placed 15px apart inside a box sized
/// for `zoom * 15`. Floored rather than allowed to reach zero, because a face
/// of 0px is not a smaller label, it is an invisible one — the same reason
/// R1653 scaled the pins.
fn canvas_font(state: &LabState, px: u32) -> u32 {
    scaled(state, px).max(6)
}

/// Derive a node's card: where every part goes, and therefore how big it is.
fn card_shape(state: &LabState, node: NodeId) -> Option<CardShape> {
    let (nx, ny) = {
        let doc = state.doc.borrow();
        let held = doc.tree(ROOT)?.node(node)?;
        (held.x, held.y)
    };
    let (x, y) = to_content(state, nx, ny);
    let w = scaled(state, card_width(state, node));
    let pad = scaled(state, 10).max(3);
    let id_font = canvas_font(state, FONT_SMALL);
    let row_font = canvas_font(state, FONT_TINY);
    let id_line = line_box(id_font);
    let row_line = line_box(row_font);
    // The header band is as tall as the identity line it holds, plus the gap
    // above and below it — not a constant that happens to fit at one zoom.
    // Tight on purpose: `line_box` already over-reserves (it is a
    // font-independent floor, not a measurement), so padding it generously
    // again compounds — measured, five lines of that made a card 9px taller
    // than the spacing the reference lays its graph out on, and the cards
    // started covering each other.
    let id_top = 2;
    let hdr = id_top + id_line + 2;
    let row_pitch = row_line + 1;
    let key_w = scaled(state, 40).max(8);
    let gap = scaled(state, 2).max(1);
    // ★ R1656 — LEVEL OF DETAIL: below the zoom at which a row's face would be
    // drawn at the legibility floor, the card shows its identity band alone.
    //
    // Derived from a real tension rather than chosen for looks. `canvas_font`
    // floors the face at 6px, because a 0px label is not a smaller label but an
    // invisible one — so below that zoom the TEXT stops shrinking while the
    // graph's spacing keeps shrinking, and a card with rows grows relative to
    // the diagram until it covers its neighbour. Measured: the press sweep found
    // 34 of 2,556 points on a card reaching a different one at the minimum zoom.
    // Every node editor this is judged against collapses a node's contents on
    // the way out for the same reason.
    //
    // ★★ R1682 — a COLLAPSE is the same request, made deliberately instead of
    // derived from the zoom, so it lands in the same place. A second path that
    // hid rows its own way would be a second answer to "what is on this card",
    // and the height — which IS the content — would be free to disagree with
    // it.
    let detailed = !card_collapsed(state, node) && scaled(state, FONT_TINY) >= 6;
    let rows: Vec<(Rect, Rect)> = card_rows(state, node)
        .iter()
        .take(if detailed { usize::MAX } else { 0 })
        .enumerate()
        .map(|(n, _)| {
            let top = hdr + u32::try_from(n).unwrap_or(0) * row_pitch;
            (
                Rect::new(pad, top, key_w, row_line),
                Rect::new(
                    pad + key_w + gap,
                    top,
                    w.saturating_sub(pad * 2 + key_w + gap).max(8),
                    row_line,
                ),
            )
        })
        .collect();
    // The height IS the content: the lowest edge any part reaches, plus the
    // bottom padding. Nothing here can disagree with what the painter draws,
    // because the painter draws exactly these rectangles.
    let content_bottom = rows
        .iter()
        .map(|(_, value)| value.y + value.h)
        .max()
        .unwrap_or(hdr);
    let badge_w = scaled(state, 38).max(10);
    let badge_font = canvas_font(state, 8);
    let badge_line = line_box(badge_font);
    Some(CardShape {
        rect: Rect::new(x, y, w, content_bottom + 3),
        id: Rect::new(
            pad,
            id_top,
            w.saturating_sub(pad * 2 + badge_w).max(8),
            id_line,
        ),
        badge: Rect::new(
            w.saturating_sub(badge_w + pad / 2),
            id_top + id_line.saturating_sub(badge_line) / 2,
            badge_w,
            badge_line,
        ),
        badge_text: Rect::new(
            w.saturating_sub(badge_w + pad / 2) + gap,
            id_top + id_line.saturating_sub(badge_line) / 2,
            badge_w.saturating_sub(gap * 2).max(4),
            badge_line,
        ),
        id_font,
        row_font,
        badge_font,
        rows,
    })
}

/// The rectangle a node's card occupies, in window pixels.
fn card_rect(state: &LabState, node: NodeId) -> Option<Rect> {
    card_shape(state, node).map(|shape| shape.rect)
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
    // ★ R1656 — the header's HALF, read from the same derivation the card is
    // built from. It was `scaled(CARD_HDR)/2` against a constant the card no
    // longer uses, which is one fact in two places by construction.
    let y = card.y + (line_box(canvas_font(state, FONT_SMALL)) / 2).max(2);
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
    /// R1678 — an affordance that puts one scope back to what it opened as.
    Reset(ResetScope),
    Config,
    Run,
    Node(NodeId),
    Pin {
        node: NodeId,
        dial: bool,
    },
    Link(LinkId),
    /// ★ R1681 — a link a source reported, which is not in the graph and so
    /// cannot be named by a [`LinkId`].
    Observed(Socket, Socket),
    /// The picked link's one act: delete it, or — when it is a reported one —
    /// take it into the drawing.
    LinkAct,
    /// One endpoint seat of the picked link's target.
    Endpoint(usize),
    /// A host frame's tab strip — its handle.
    Frame(NodeId),
    /// ★★ R1682 — one of the selected card's own three acts.
    NodeAct(NodeAct),
    /// ★★ R1683 — the seat that opens the one text field on the card's name.
    Rename,
    /// The seat that opens it on a configuration path instead.
    AddKey,
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
            // ★ R1682 — the node's-life seats first: they sit above the form in
            // the same scrolling body, and the form's own rows begin below
            // them, so the two cannot overlap — but asking in painted order is
            // what keeps that true if either moves.
            if state.selected.get().is_some() {
                for act in NodeAct::ALL {
                    if contains(node_act_seat(state, act), px, py) {
                        return Self::NodeAct(act);
                    }
                }
                // ★ R1683 — the rename seat. The FIELD beside it is a real
                // external and owns its own hit target, so it is deliberately
                // not an arm here: a press inside it reaches the field's own
                // router, which is what puts the caret where the pointer is.
                let (box_rect, apply, key) = rename_row();
                if contains(in_body(state, apply), px, py) {
                    return Self::Rename;
                }
                // ★ R1683 — the shut box is the field's own seat. It looks like
                // somewhere to type, so pressing it has to open the thing it
                // looks like; while the field IS open the real external is
                // painted there and owns the press, which is what puts the
                // caret where the pointer landed.
                if state.editing.get().is_none() && contains(in_body(state, box_rect), px, py) {
                    return Self::Rename;
                }
                if contains(in_body(state, key), px, py) {
                    return Self::AddKey;
                }
            }
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
            // ★ R1662 — the palette body SCROLLS, so a press has to be asked
            // in the frame the rows are stated in. Every rectangle here
            // (`palette_row`, `discovery_rect`) is written in the unscrolled
            // window frame, which is where the painter also reads it, so
            // folding the offset into the QUERY keeps one set of rectangles
            // rather than two. Without this the paint moved and the hit test
            // did not: R1662's end-to-end probe pressed the centre of the
            // scrolled-to `Querier` row and the screen answered `Publisher`,
            // which is the R1656 class exactly.
            let (px, py) = in_pane(&state.palette_scroll, px, py);
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
            if contains(view_reset_rect(), px, py) {
                return Self::Reset(ResetScope::View);
            }
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
        // ★ R1678 — the gate panel's reset row, BEFORE the canvas: the panel
        // floats over the canvas, so a press inside it that fell through to
        // the world would pan the graph out from under the button.
        for (scope, seat) in reset_seats(state) {
            if contains(seat, px, py) {
                return Self::Reset(scope);
            }
        }
        if contains(canvas_rect(), px, py) {
            return Self::on_canvas(state, px, py);
        }
        Self::Nothing
    }

    /// What a press inside the canvas viewport reaches.
    fn on_canvas(state: &LabState, px: u32, py: u32) -> Self {
        // ★ The canvas is a viewport onto a world surface, so a press is
        // resolved in the surface's coordinates — the same ones the painter
        // places cards in. One conversion, at the boundary.
        let (cx, cy) = window_to_content(state, px, py);
        // ★★ R1681 — the picked link's own affordances, before everything else
        // on the canvas: they float over the graph, and a press that fell
        // through to the world would pan the canvas out from under the button.
        // The rectangles are the PAINTER's, read from one derivation, so "it is
        // drawn there" and "it is pressed there" cannot be two answers.
        if let Some(chrome) = link_chrome(state) {
            if holds(chrome.act, cx, cy) {
                return Self::LinkAct;
            }
            for (n, (_, seat)) in chrome.chips.iter().enumerate() {
                if holds(*seat, cx, cy) {
                    return Self::Endpoint(n);
                }
            }
        }
        // Pins before cards: a pin overhangs its card's edge, and the pin is
        // the smaller target, so testing the card first would make a link
        // impossible to author with a real mouse.
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
        // Reported links AFTER drawn ones: where the two run together the drawn
        // one is the one somebody made a decision about.
        if let Some((from, to)) = observed_at(state, cx, cy) {
            return Self::Observed(from, to);
        }
        // The frame's TAB, not its interior: the interior is where the cards
        // are, and a group that swallowed presses over its own members would
        // make a node undraggable the moment it joined one.
        for (id, _) in frames_of(state) {
            let r = frame_rect_of(state, id);
            let tab = Rect::new(r.x, r.y, r.w, scaled(state, FRAME_TAB).max(10));
            if holds(tab, cx, cy) {
                return Self::Frame(id);
            }
        }
        Self::Canvas
    }

    /// The word the wire answers a press with.
    fn word(&self, state: &LabState) -> String {
        match self {
            Self::Nothing => "nothing".into(),
            Self::Rail(name) => format!("rail:{name}"),
            Self::Role(role) => format!("role:{}", role.name()),
            Self::DiscoveryToggle => "discovery".into(),
            Self::Zoom(up) => format!("zoom:{}", if *up { "in" } else { "out" }),
            Self::Reset(scope) => format!("reset:{}", scope.wire()),
            Self::Config => "config".into(),
            Self::Run => "run".into(),
            Self::Node(id) => format!("node:{}", state.name_of(*id)),
            Self::Pin { node, dial } => format!(
                "pin:{}:{}",
                state.name_of(*node),
                if *dial { "dial" } else { "accept" }
            ),
            Self::Link(id) => format!("link:{}", id.0),
            Self::Observed(from, to) => format!(
                "observed:{}>{}",
                state.name_of(from.node),
                state.name_of(to.node)
            ),
            Self::LinkAct => "link:act".into(),
            Self::Endpoint(n) => format!("link:endpoint:{n}"),
            Self::Frame(id) => format!(
                "frame:{}",
                state.frames.borrow().get(id).cloned().unwrap_or_default()
            ),
            // R1682 — named by the act rather than by the card, because the
            // card is whatever is selected and the wire reads that separately.
            Self::NodeAct(act) => format!("card:{}", act.wire()),
            Self::Rename => "card:rename".into(),
            Self::AddKey => "card:addkey".into(),
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

/// The reported link whose wire passes within a few pixels of the cursor
/// (R1681).
///
/// The same chord sampling as [`link_at`], over the other layer. Two functions
/// and not one because what they answer with is different — an observation has
/// no id — and folding them together would mean inventing one.
fn observed_at(state: &LabState, px: i64, py: i64) -> Option<(Socket, Socket)> {
    for seen in state.doc.borrow().observations(ROOT) {
        let (Some(a), Some(b)) = (
            card_rect(state, seen.from.node),
            card_rect(state, seen.to.node),
        ) else {
            continue;
        };
        let (ax, ay) = centre(pin_rect(state, a, true));
        let (bx, by) = centre(pin_rect(state, b, false));
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
                return Some((seen.from, seen.to));
            }
        }
    }
    None
}

/// The drawn link landing on `node`'s accept pin that a cursor at `at` is
/// nearest to — the one a press on that pin picks up (R1681).
///
/// The reference's rule, and the reason it is the nearest and not the first: an
/// accept pin can hold several wires and the one a person means is the one they
/// are pointing at. A **reported** link is deliberately not eligible — it is
/// not in the drawing, so there is nothing to pick up.
fn link_into_pin(state: &LabState, node: NodeId, at: (i64, i64)) -> Option<LinkId> {
    let doc = state.doc.borrow();
    let mut best: Option<(u64, LinkId)> = None;
    for link in doc.tree(ROOT)?.links() {
        if link.to.node != node {
            continue;
        }
        let Some(card) = card_rect(state, link.from.node) else {
            continue;
        };
        let (ax, ay) = centre(pin_rect(state, card, true));
        let reach = at.0.abs_diff(i64::from(ax)).pow(2) + at.1.abs_diff(i64::from(ay)).pow(2);
        if best.is_none_or(|(held, _)| reach < held) {
            best = Some((reach, link.id));
        }
    }
    best.map(|(_, id)| id)
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
///
/// ★ R1678 — it grows a row when there is something to put back. The height is
/// derived from [`changed_scopes`] rather than reserved, because a permanently
/// reserved strip is a band of empty panel on the screen a person spends the
/// most time looking at, and the reference makes the same choice (its reset
/// affordances are conditional, not disabled).
fn gate_rect(state: &LabState) -> Rect {
    let canvas = canvas_rect();
    let lines = u32::try_from(state.gate_lines().len()).unwrap_or(0) + 1;
    let resets = u32::from(!changed_scopes(state).is_empty()) * RESET_ROW_H;
    let h = 34 + lines * 20 + resets;
    Rect::new(
        canvas.x + canvas.w - 262,
        canvas.y + canvas.h - h - 12,
        250,
        h,
    )
}

/// The height the reset row adds to the gate panel: the buttons plus the gap
/// above them.
const RESET_ROW_H: u32 = 32;
/// One reset button's height.
const RESET_BTN_H: u32 = 22;

/// Where each reset affordance sits, window-absolute — **the one list the paint
/// and the hit test both read**.
///
/// R1651.1 is why it is one list: that round painted the option chips
/// content-hugging and hit-tested them by equal division, so the second chip
/// answered for the first. A seat computed twice is two layouts.
fn reset_seats(state: &LabState) -> Vec<(ResetScope, Rect)> {
    let scopes = changed_scopes(state);
    if scopes.is_empty() {
        return Vec::new();
    }
    let gate = gate_rect(state);
    let inner = gate.w - 24;
    let gap = 6;
    let count = u32::try_from(scopes.len()).unwrap_or(1);
    let each = (inner + gap).saturating_sub(gap * count) / count;
    let y = gate.y + gate.h - RESET_BTN_H - 8;
    scopes
        .into_iter()
        .enumerate()
        .map(|(n, scope)| {
            let n = u32::try_from(n).unwrap_or(0);
            (
                scope,
                Rect::new(gate.x + 12 + n * (each + gap), y, each, RESET_BTN_H),
            )
        })
        .collect()
}

/// The view reset's seat in the toolbar's zoom cluster.
///
/// Unconditional, beside the zoom controls, which is where the reference keeps
/// it — see [`ResetScope::gated`] for why this one is not on the panel.
fn view_reset_rect() -> Rect {
    let out = zoom_rect(false);
    Rect::new(out.x - 40, out.y, 34, 24)
}

fn hint_rect() -> Rect {
    let canvas = canvas_rect();
    // ★ R1656 — clamped to the pane it sits in. It was a flat 470, so on a
    // canvas narrower than that the strip advertising the screen's gestures was
    // painted over the inspector beside it.
    let w = 470.min(canvas.w.saturating_sub(24)).max(80);
    Rect::new(canvas.x + 12, canvas.y + canvas.h - 34, w, 24)
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
///
/// R1682 moved it down by one row: the node's-life seats sit between the degree
/// box and the form. R1683 moved it down by another: the one text field and the
/// seat that opens it sit under those.
const INSP_HEAD_H: u32 = 160;

/// ★★ R1682 — what a person can do to the selected card itself.
///
/// The reference puts exactly these in its inspector beside the node's
/// identity, which is the right place for the same reason it gives: they are
/// the only affordances that act on the *card* rather than on one of its
/// fields, and a canvas gesture for them would collide with placing and wiring,
/// the two things a press on a card already means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NodeAct {
    /// Draw it small, or full size again.
    Collapse,
    /// Switch it off, or back on.
    Disable,
    /// Take it off the canvas.
    Delete,
}

impl NodeAct {
    /// The census. Consumers iterate this rather than re-listing the arms.
    const ALL: [Self; 3] = [Self::Collapse, Self::Disable, Self::Delete];

    /// The word a press on this seat answers with, and the action that does the
    /// same thing — one name, so the two channels cannot drift.
    const fn wire(self) -> &'static str {
        match self {
            Self::Collapse => "collapse",
            Self::Disable => "disable",
            Self::Delete => "delete_node",
        }
    }

    /// The tag the seat is painted under, which is also what a driver presses.
    const fn tag(self) -> &'static str {
        match self {
            Self::Collapse => "lab.inspector.collapse",
            Self::Disable => "lab.inspector.disable",
            Self::Delete => "lab.inspector.delete",
        }
    }

    /// What the seat says, given what the card is doing now.
    ///
    /// The two toggles name the act they would perform rather than the state
    /// the card is in — the reference's own choice, and the one that makes a
    /// button readable without first working out which way round it is.
    fn word(self, collapsed: bool, disabled: bool) -> &'static str {
        match self {
            Self::Collapse if collapsed => "expand",
            Self::Collapse => "collapse",
            Self::Disable if disabled => "switch on",
            Self::Disable => "switch off",
            Self::Delete => "delete",
        }
    }

    /// Where the seat sits in the frame the inspector's body is drawn in — the
    /// same frame the identity labels above it and the form below it use, so
    /// the whole pane scrolls as one thing.
    fn local_seat(self) -> Rect {
        let n = Self::ALL.iter().position(|a| *a == self).unwrap_or(0);
        let width = (INSP_W - PAD * 2 - NODE_ACT_GAP * 2) / 3;
        let step = u32::try_from(n).unwrap_or(0) * (width + NODE_ACT_GAP);
        Rect::new(PAD + step, NODE_ACT_Y, width, NODE_ACT_H)
    }
}

/// A node's-life seat in WINDOW coordinates — where a pointer meets it.
///
/// Derived from the painted rectangle by the pane's placement and its scroll
/// offset. A second set of rectangles written in window coordinates is how the
/// paint and the gesture come to disagree once the pane is scrolled (R1662).
///
/// ★ [`PANEL_FRAME`] is part of the transform and was missing from the first
/// draft: the seats are painted inside the panel's border, so the body's origin
/// is the panel's origin plus its frame. Measured — the seat answered one pixel
/// left and one pixel up of where the layout put it, which a 90-wide seat
/// absorbs and a narrow one would not.
fn node_act_seat(state: &LabState, act: NodeAct) -> Rect {
    in_body(state, act.local_seat())
}

/// A rectangle stated in the inspector body's own frame, in WINDOW coordinates.
///
/// The one transform every affordance in that pane goes through, so a second
/// one cannot be written in window coordinates and drift once the pane scrolls
/// (R1662). [`PANEL_FRAME`] is part of it: the body is drawn inside the panel's
/// border.
fn in_body(state: &LabState, local: Rect) -> Rect {
    let pane = inspector_rect();
    let (ox, oy) = state.inspector_scroll.offset();
    let shift = |v: u32, base: u32, by: i32| -> u32 {
        u32::try_from(
            (i64::from(v) + i64::from(base) + i64::from(PANEL_FRAME) - i64::from(by)).max(0),
        )
        .unwrap_or(0)
    };
    Rect::new(
        shift(local.x, pane.x, ox),
        shift(local.y, pane.y, oy),
        local.w,
        local.h,
    )
}

/// ★★ R1683 — where the one text field sits in the inspector's body, and the
/// seat that opens it on the card's name.
///
/// Under the node's-life row, because it IS a node's-life operation — the one
/// that needs a value typed. The reference puts its name box in the same place
/// for the same reason.
fn rename_row() -> (Rect, Rect, Rect) {
    let width = INSP_W - PAD * 2;
    let apply = seat_w("rename");
    let key = seat_w("+ key");
    let box_w = width.saturating_sub(apply + key + NODE_ACT_GAP * 2);
    (
        Rect::new(PAD, EDIT_ROW_Y, box_w, NODE_ACT_H),
        Rect::new(PAD + box_w + NODE_ACT_GAP, EDIT_ROW_Y, apply, NODE_ACT_H),
        Rect::new(
            PAD + box_w + apply + NODE_ACT_GAP * 2,
            EDIT_ROW_Y,
            key,
            NODE_ACT_H,
        ),
    )
}

/// How wide a seat holding this word is, at the inspector's small face.
fn seat_w(word: &str) -> u32 {
    u32::try_from(word.len()).unwrap_or(6) * FONT_SMALL * 7 / 10 + 16
}

/// How far down the inspector's body the text field's row sits.
const EDIT_ROW_Y: u32 = 134;

/// The node's-life row: how far down the inspector it sits, how tall its seats
/// are, and the gap between them.
const NODE_ACT_Y: u32 = 112;
/// How tall a node's-life seat is.
///
/// ★ R1683 trimmed it from 24 to 20, and the reason is a measurement rather
/// than taste: the head grew twice this session, and at 24 the settings form's
/// add-a-key chips fell below the pane's fold on the opening screen. The pane
/// scrolls, so nothing was unreachable — but a person should not have to
/// scroll to reach an affordance the screen opens with.
const NODE_ACT_H: u32 = 20;
/// The gap between two node's-life seats.
const NODE_ACT_GAP: u32 = 6;

/// The form's geometry in the frame the PAINTER draws it in: inside the
/// inspector's scrolling body, so it rides the scroll instead of being shifted
/// by hand every time the offset moves.
fn inspector_geometry_local(state: &LabState) -> FormGeometry {
    let form = selected_form(state).unwrap_or_default();
    form_geometry(&form, (PAD, INSP_HEAD_H), &form_style())
}

/// The same geometry in WINDOW coordinates — where a pointer meets it and
/// where assistive technology is told it is.
///
/// ★ R1662 — derived from the pane-local one by the pane's placement and its
/// current scroll offset, through the one translation
/// [`FormGeometry::translated`] owns. Computing it a second time here is how
/// the paint and the gesture come to disagree, and a row the scroll has carried
/// off the top is dropped rather than reported at the edge.
fn inspector_geometry(state: &LabState) -> FormGeometry {
    let rect = inspector_rect();
    let (ox, oy) = state.inspector_scroll.offset();
    inspector_geometry_local(state).translated(
        i32::try_from(rect.x).unwrap_or(i32::MAX) - ox,
        i32::try_from(rect.y).unwrap_or(i32::MAX) - oy,
    )
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

/// The width of the outline [`panel`] and [`box_at`] stroke INSIDE their box.
///
/// Named so [`panel_content`] and the border below are one number: a content
/// inset that remembers the frame's width separately from the frame is an
/// inset that goes wrong the day the frame changes.
const PANEL_FRAME: u32 = 1;

/// A bordered panel's CONTENT rectangle in its own space: its box less the
/// [`PANEL_FRAME`] outline [`panel`] draws inside it.
///
/// ★ R1672 — the placement half of
/// [`pinion_core::containment::content_rect`], which is the check half. A pane
/// that handed its scrolling body `(0, 0, rect.w, rect.h)` put the body over
/// its own outline, and the channel could not say so until it learned the
/// border-box / content-box distinction. Named here so the two halves cannot
/// drift: change the frame's width and both follow.
fn panel_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PANEL_FRAME)),
        // A plain panel reserves no band of itself: it draws a frame and gives
        // everything inside it away. R1674 made this an argument rather than a
        // default so a panel that GROWS a header has to come back here.
        &[],
    )
}

fn panel(tag: &str, rect: Rect, fill: Color, border: Option<Color>, children: Vec<Scene>) -> Scene {
    let mut style = BoxStyle::filled(fill);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, PANEL_FRAME));
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
    dashed_wire(tag, from, to, colour, width, None)
}

/// The same wire under a dash rhythm (R1681.2).
///
/// ★★ The rhythm is the framework's, not a second one: `Stroke` has carried
/// `dash` since R1575 and the sibling screen that draws the same two layers
/// already spells a reported link `Dash::DOTTED`. This screen drew its reported
/// links in the warning colour alone and R1681 wrote down "the wire primitive
/// carries no dash pattern" — which was false, and false in the direction that
/// invents a limit instead of reaching for what is there.
fn dashed_wire(
    tag: &str,
    from: (u32, u32),
    to: (u32, u32),
    colour: Color,
    width: u32,
    dash: Option<Dash>,
) -> Scene {
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
            PathStyle::stroked(match dash {
                Some(rhythm) => Stroke::new(colour, width).with_dash(rhythm),
                None => Stroke::new(colour, width),
            }),
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
    for (n, (name, reserved_for)) in spec::RAIL.iter().enumerate() {
        let seat = local(rail_seat(n));
        let active = *name == spec::RAIL_ACTIVE;
        let mut box_node = box_at(
            &format!("lab.rail.{name}"),
            seat,
            if active { ink.accent_soft } else { ink.surface },
            Some(if active { ink.accent_line } else { ink.surface }),
            10,
        );
        // ★ R1669 — a reserved seat is DECLARED unavailable with its booking,
        // not merely drawn in a dimmer ink. The declaration is what makes it
        // inert to the pointer, fades it, announces the reason to a screen
        // reader and puts it on `scene/disabled`; the dim ink did one of those
        // four and nothing could check it.
        if let Some(why) = reserved_for
            && let Some(layout) = box_node.layout_style_mut()
        {
            *layout = layout.clone().with_unavailable(Unavailable::reserved(*why));
        }
        children.push(box_node);
        children.extend(rail_icon(
            name,
            seat,
            if active {
                ink.accent
            } else if reserved_for.is_some() {
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
    // ★ R1662 — the pane scrolls. Its content is taller than any window this
    // screen declares a floor for, and before this the overflow was simply
    // painted past the bottom edge: `scene/scroll_reach` reported the last
    // rows `lost`, meaning no gesture of any kind reached them. The extent is
    // derived from `children` by the pane rather than declared here, so it
    // cannot go stale as rows are added
    // ([[debt-the-node-lab-panes-do-not-scroll]]).
    panel(
        "lab.palette",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![scroll_pane(
            &state.palette_scroll,
            panel_content(rect),
            (0, PAD),
            // Every press on this screen belongs to the one root `External`
            // that does the screen's own hit test, so the pane must be
            // invisible to the router (R1655).
            PanePointer::PassesThrough,
            children,
        )],
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
    /// The clearance the launch chip keeps between its frame and its word,
    /// above and below.
    const GATE_PAD: u32 = 1;
    /// The same, left and right — wider, because a word set flush against a
    /// vertical rule reads as touching it long before it overlaps.
    const GATE_TEXT_PAD: u32 = 9;
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
        //
        // ★★ R1672 — and the chip's height and the word's seat are both DERIVED
        // from the line box and the frame. They were a picked `22` and a picked
        // `y = 4`, which happened to put the word's last row on the chip's own
        // outline: exactly one pixel, on the bottom edge, invisible until
        // `containment` learned that a border is ink the box owns. The numbers
        // below come out at the same 22 — the pixels do not move, the way they
        // are arrived at does.
        {
            let line = line_box(FONT_SMALL);
            let seat = Rect::new(PAD + 300, 12, 104, line + (PANEL_FRAME + GATE_PAD) * 2);
            let inner = panel_content(seat);
            panel(
                "lab.toolbar.gate",
                seat,
                ink.raised,
                Some(gate_colour),
                vec![label(
                    gate_word,
                    // ★ R1656 — the LINE box of the face, not the face's size.
                    Rect::new(
                        inner.x + GATE_TEXT_PAD,
                        inner.y + GATE_PAD,
                        inner.w.saturating_sub(GATE_TEXT_PAD * 2),
                        line,
                    ),
                    FONT_SMALL,
                    gate_colour,
                )],
            )
        },
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
    // ★ R1678 — unconditional, beside the zoom controls, which is where the
    // reference keeps the view reset. See `ResetScope::gated`.
    let view_reset = local(view_reset_rect());
    children.push(box_at(
        "lab.reset.view",
        view_reset,
        ink.raised,
        Some(ink.outline),
        6,
    ));
    children.push(label(
        "home",
        Rect::new(view_reset.x + 4, view_reset.y + 6, 28, 12),
        FONT_SMALL,
        ink.text_3,
    ));
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
    // ★★★ R1681.3 — the picked link's affordances paint LAST, because they are
    // what a press over them reaches.
    //
    // They were painted with the wires, which is where they belong visually and
    // is wrong: a card drawn afterwards covers them, while `Hit::at` tests them
    // FIRST. So the screen showed a card and answered `link:act` for the same
    // pixel — the paint and the hit test disagreeing about which thing is on
    // top, which is this screen's oldest defect class and the reason R1656
    // exists. Found by looking at the running app, not by any check here.
    if let Some(chrome) = link_chrome(state) {
        children.extend(link_affordances(&chrome, ink));
    }
    children
}

/// The reference's dot grid: a pip every 22 canvas units, moving with the pan
/// so the canvas reads as a surface being moved rather than a viewport sliding
/// over a static picture.
/// How big the world surface is, at this zoom and this window.
///
/// ★ R1656 — ONE derivation, because two of them disagreed. The surface sized
/// itself here while the pip texture on it ran to the VIEWPORT's width, and at
/// the minimum zoom the world is narrower than the viewport — so the texture
/// marched thousands of dots past the edge of the thing it was decorating. The
/// containment check reported 11,472 of them the first time the size axis
/// visited "zoomed out, maximised"; before that nothing looked, because a pip
/// carries no tag and every other gate here is tag-keyed.
fn world_extent(state: &LabState) -> (u32, u32) {
    let rect = canvas_rect();
    (
        scaled(state, WORLD.unsigned_abs()).max(rect.w),
        scaled(state, WORLD.unsigned_abs()).max(rect.h),
    )
}

fn canvas_grid(state: &LabState, ink: Ink) -> Vec<Scene> {
    let rect = canvas_rect();
    let (world_w, world_h) = world_extent(state);
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
    while gy < (from_y + rect.h + pitch).min(world_h) {
        let mut gx = from_x;
        while gx < (from_x + rect.w + pitch).min(world_w) {
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

/// Where the picked link's own affordances sit, in the world surface's
/// coordinates (R1681).
///
/// ★ ONE authority, read by the painter and by the hit test. R1653 found three
/// consecutive rounds of defects that hid because a control's paint and its
/// press were two arithmetics that agreed until they did not; every rectangle
/// on this screen that a person aims at comes from a single derivation for that
/// reason, and these three are no different.
struct LinkChrome {
    /// The type size every part of it is drawn at, and therefore the size every
    /// part of it is derived FROM.
    font: u32,
    /// The endpoint caption at the wire's middle.
    label: Rect,
    /// The word the label carries.
    caption: String,
    /// One seat per endpoint the target listens on — **empty unless there is
    /// more than one**, because a choice between one thing is not a choice, and
    /// the reference draws the row only when the target listens twice.
    chips: Vec<(String, Rect)>,
    /// Which of those the link took.
    current: usize,
    /// The delete-or-adopt seat below the wire.
    act: Rect,
    /// Whether that seat adopts (a reported link) rather than deletes (a drawn
    /// one). The reference's one button with two meanings, and it is one button
    /// because the two are the same question — *should this link be in the
    /// drawing* — answered from opposite sides.
    adopt: bool,
}

/// The chrome of whichever link is picked, or `None` when none is.
fn link_chrome(state: &LabState) -> Option<LinkChrome> {
    let pick = state.selected_link.get()?;
    let (from_socket, to_socket, adopt) = match pick {
        LinkPick::Authored(id) => {
            let link = state.doc.borrow().tree(ROOT)?.link(id).copied()?;
            (link.from, link.to, false)
        }
        LinkPick::Observed(from, to) => (from, to, true),
    };
    let dials = card_rect(state, from_socket.node)?;
    let accepts = card_rect(state, to_socket.node)?;
    let from = centre(pin_rect(state, dials, true));
    let to = centre(pin_rect(state, accepts, false));
    let mid = (u32::midpoint(from.0, to.0), u32::midpoint(from.1, to.1));

    let endpoints = endpoints_of(state, to_socket.node);
    let taken = endpoint_at(state, to_socket);
    let current = taken
        .as_ref()
        .and_then(|one| endpoints.iter().position(|e| e == one))
        .unwrap_or(0);
    // The caption is the endpoint the link took — which is the whole point of
    // there being an endpoint per link rather than per node.
    let caption = taken
        .or_else(|| endpoints.first().cloned())
        .unwrap_or_default();

    // ★★ R1681 — the chrome SCALES with the canvas, and every box is derived
    // from the type size rather than sized beside it. Both halves have a round
    // behind them. R1653: a part of the diagram held at fixed pixels keeps them
    // while the cards shrink, and at low zoom it covers a card the pointer can
    // then never reach — measured here as a `delete` seat swallowing the whole
    // of one card at the zoomed-out sweep. R1656: a box sized by a number
    // somebody typed, beside a run sized by the shaper, is two derivations of
    // one fact and the run wins.
    let font = canvas_font(state, FONT_SMALL);
    let line = line_box(font);
    let pad = (font / 2).max(3);
    let seat_h = line + pad;
    // Seven tenths of the type size per character, in the spirit of `line_box`:
    // over-reserve a little rather than clip a run. Both bounds either side of
    // this were MEASURED on the running screen — a whole em made a sixteen-
    // character address a seat wider than the card it belongs to, and three
    // fifths elided `tcp/0.0.0.0:7447` down to `…0.7447`, losing the scheme,
    // which is the half that carries the transport.
    let seat_w =
        |text: &str| -> u32 { u32::try_from(text.len()).unwrap_or(8) * font * 7 / 10 + pad * 2 };
    let gap = (font / 2).max(2);

    let mut chips = Vec::new();
    if endpoints.len() > 1 {
        let total: u32 = endpoints
            .iter()
            .map(|e| seat_w(e) + gap)
            .sum::<u32>()
            .saturating_sub(gap);
        let mut left = mid.0.saturating_sub(total / 2);
        let top = mid.1.saturating_sub(seat_h * 2 + line + gap);
        for one in &endpoints {
            let width = seat_w(one);
            chips.push((one.clone(), Rect::new(left, top, width, seat_h)));
            left += width + gap;
        }
    }

    let label_w = seat_w(&caption).max(seat_w("delete"));
    let label = Rect::new(
        mid.0.saturating_sub(label_w / 2),
        mid.1.saturating_sub(seat_h + line / 2),
        label_w,
        seat_h,
    );
    let act = Rect::new(
        mid.0.saturating_sub(seat_w("delete") / 2),
        mid.1 + line / 2,
        seat_w("delete"),
        seat_h,
    );

    // ★★★ R1681.3 — the column sits ON its wire, and is only moved to stay
    // inside what the canvas is showing.
    //
    // R1681 moved it clear of every card as well, to satisfy this screen's
    // invariant that a press ANYWHERE on a card reaches that card (R1655). The
    // running screen is what showed the price: measured, the picked link's
    // label and its `delete` seat ended up **240 pixels below the wire they
    // belong to**, past two other cards, because up and down were blocked at
    // every step until they were not. An annotation that far from what it
    // annotates is not an annotation.
    //
    // So the invariant is the thing that gives, and precisely: a press covered
    // by the PICKED LINK'S OWN CHROME is not an unexplained hole in a card, it
    // is an affordance the person summoned by picking that link, and the
    // reference draws it exactly there. The gate learns that exception from
    // THIS function rather than from a list beside it, so a chrome that wandered
    // somewhere absurd would still be caught covering cards it has no business
    // covering. What is left unmoved is the viewport clamp — an affordance
    // nudged off-screen would trade one unreachable thing for another, which is
    // what the first draft of R1681 measured itself doing.
    let mut parts: Vec<Rect> = chips
        .iter()
        .map(|(_, seat)| *seat)
        .chain([label, act])
        .collect();
    let shift = placement(state, &parts, line.max(2));
    for part in &mut parts {
        *part = Rect::new(
            part.x.saturating_add_signed(shift.0),
            part.y.saturating_add_signed(shift.1),
            part.w,
            part.h,
        );
    }
    let act = parts.pop().unwrap_or(act);
    let label = parts.pop().unwrap_or(label);

    Some(LinkChrome {
        font,
        label,
        caption,
        chips: chips
            .into_iter()
            .zip(parts)
            .map(|((one, _), seat)| (one, seat))
            .collect(),
        current,
        act,
        adopt,
    })
}

/// What the picked link carries: its caption, its endpoint seats and its one
/// act (R1681).
///
/// ★ The text of each seat is that seat's CHILD, not its neighbour. A floating
/// annotation is drawn over the diagram on purpose, so it is its own layer, and
/// saying that structurally is what keeps "no two runs of one widget overlap"
/// true without an exception list beside the rule.
fn link_affordances(chrome: &LinkChrome, ink: Ink) -> Vec<Scene> {
    // ★ R1656 — every run's box is the LINE BOX the shaper produces at this
    // type size, not a number the author guessed. One helper, so the three
    // seats cannot be sized three ways.
    let inner = |seat: Rect| -> Rect {
        let pad = (chrome.font / 2).max(3);
        Rect::new(
            pad,
            (seat.h.saturating_sub(line_box(chrome.font))) / 2,
            seat.w.saturating_sub(pad * 2),
            line_box(chrome.font),
        )
    };
    let mut out = vec![panel(
        "lab.link.label",
        chrome.label,
        ink.accent_soft,
        Some(ink.accent_line),
        vec![tagged_label(
            "lab.link.label.text",
            chrome.caption.clone(),
            inner(chrome.label),
            chrome.font,
            ink.accent,
        )],
    )];
    for (n, (endpoint, seat)) in chrome.chips.iter().enumerate() {
        let picked = n == chrome.current;
        out.push(panel(
            &format!("lab.link.endpoint.{n}"),
            *seat,
            ink.surface,
            Some(if picked { ink.accent } else { ink.outline }),
            vec![tagged_label(
                &format!("lab.link.endpoint.{n}.text"),
                endpoint.clone(),
                inner(*seat),
                chrome.font,
                if picked { ink.accent } else { ink.text_2 },
            )],
        ));
    }
    let (word, edge) = if chrome.adopt {
        ("adopt", ink.warn)
    } else {
        ("delete", ink.err)
    };
    out.push(panel(
        "lab.link.act",
        chrome.act,
        ink.surface,
        Some(edge),
        vec![tagged_label(
            "lab.link.act.text",
            word.to_owned(),
            inner(chrome.act),
            chrome.font,
            edge,
        )],
    ));
    out
}

/// Whether the picked link's own chrome covers this window point (R1681.3).
///
/// ★★ The exception the card sweeps take, derived from the function that PAINTS
/// the chrome rather than restated beside them. A summoned overlay covering
/// part of a card is what the reference does and what a person expects; a
/// screen that moved the overlay instead put it 240 pixels from the wire it
/// annotates, which is the thing this exception exists to avoid re-doing.
#[cfg(test)]
fn chrome_covers(state: &LabState, px: u32, py: u32) -> bool {
    let Some(chrome) = link_chrome(state) else {
        return false;
    };
    let (cx, cy) = window_to_content(state, px, py);
    holds(chrome.act, cx, cy)
        || holds(chrome.label, cx, cy)
        || chrome.chips.iter().any(|(_, seat)| holds(*seat, cx, cy))
}

/// Where the picked link's column goes, as an offset from the wire's middle
/// (R1681, narrowed R1681.3).
///
/// One rule: it must be inside what the canvas is showing. Answers `(0, 0)`
/// whenever the reference's own placement — on the wire — already fits, which
/// is nearly always.
fn placement(state: &LabState, parts: &[Rect], step: u32) -> (i32, i32) {
    let canvas = canvas_rect();
    let (ox, oy) = world_offset(state, state.pan.get());
    let shown = Rect::new(
        u32::try_from(ox).unwrap_or(0),
        u32::try_from(oy).unwrap_or(0),
        canvas.w,
        canvas.h,
    );
    let moved = |part: &Rect, by: (i32, i32)| -> Rect {
        Rect::new(
            part.x.saturating_add_signed(by.0),
            part.y.saturating_add_signed(by.1),
            part.w,
            part.h,
        )
    };
    let inside = |by: (i32, i32)| {
        parts.iter().all(|part| {
            let r = moved(part, by);
            r.x >= shown.x
                && r.y >= shown.y
                && r.x + r.w <= shown.x + shown.w
                && r.y + r.h <= shown.y + shown.h
        })
    };
    // The horizontal nudge is a clamp, not a search: a column wider than the
    // gap it sits in has one place to be, against whichever edge crowds it.
    let left = parts.iter().map(|p| p.x).min().unwrap_or(shown.x);
    let right = parts.iter().map(|p| p.x + p.w).max().unwrap_or(shown.x);
    let dx = if left < shown.x {
        i32::try_from(shown.x - left).unwrap_or(0)
    } else if right > shown.x + shown.w {
        -i32::try_from(right - (shown.x + shown.w)).unwrap_or(0)
    } else {
        0
    };

    // Vertically it moves only when it has to, and then by whole rows so the
    // column does not creep pixel by pixel as the graph is panned.
    let step = i32::try_from(step).unwrap_or(2).max(2);
    (0..64i32)
        .flat_map(|off| [(dx, -off * step), (dx, off * step)])
        .find(|by| inside(*by))
        .unwrap_or((dx, 0))
}

/// The wires — drawn and reported — and the affordances the picked one carries.
fn canvas_wires(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();
    let selected_link = state.selected_link.get();
    // A link being re-aimed is drawn following the cursor instead of where it
    // still is, so the person sees where it is going rather than where it was.
    let moving = match state.drag.get() {
        Some(Drag::Rewire { link, .. }) => Some(link),
        _ => None,
    };
    {
        let doc = state.doc.borrow();
        if let Some(tree) = doc.tree(ROOT) {
            for link in tree.links() {
                if moving == Some(link.id) {
                    continue;
                }
                let (Some(a), Some(b)) = (
                    card_rect(state, link.from.node),
                    card_rect(state, link.to.node),
                ) else {
                    continue;
                };
                let chosen = selected_link == Some(LinkPick::Authored(link.id));
                let from = centre(pin_rect(state, a, true));
                let to = centre(pin_rect(state, b, false));
                children.push(wire(
                    &format!("lab.link.{}", link.id.0),
                    from,
                    to,
                    if chosen { ink.accent } else { ink.accent_line },
                    if chosen { 2 } else { 1 },
                ));
            }
        }
        // ★★ R1681 — what a source reported: the warning colour AND the dash
        // rhythm, because it is not in the graph and must not read as though it
        // were. ★ R1681.2 — the rhythm is `Dash::DOTTED`, which is what the
        // sibling screen drawing these same two layers already uses for a
        // reported link; R1681 said this primitive had no dash and reached for
        // colour alone, which was false.
        for seen in doc.observations(ROOT) {
            let (Some(a), Some(b)) = (
                card_rect(state, seen.from.node),
                card_rect(state, seen.to.node),
            ) else {
                continue;
            };
            let chosen = selected_link == Some(LinkPick::Observed(seen.from, seen.to));
            children.push(dashed_wire(
                &format!(
                    "lab.observed.{}.{}",
                    state.name_of(seen.from.node),
                    state.name_of(seen.to.node)
                ),
                centre(pin_rect(state, a, true)),
                centre(pin_rect(state, b, false)),
                ink.warn,
                if chosen { 3 } else { 2 },
                Some(Dash::DOTTED),
            ));
        }
    }

    // A link in flight follows the cursor, so a drag shows what it will do
    // before it does it — the reference commits on release. ★ R1681 — ONE
    // block for both drags: authoring and re-aiming draw the same preview from
    // the same pin, and the only difference is whether a wire already exists at
    // the far end. Two copies of it is how the second one would come to be
    // drawn from somewhere else.
    let in_flight = match state.drag.get() {
        Some(Drag::Wire { from } | Drag::Rewire { from, .. }) => Some(from),
        _ => None,
    };
    if let Some(from) = in_flight
        && let Some(card) = card_rect(state, from)
    {
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

    children
}

/// One card per node: its identity band, its digest rows, and its pins.
///
/// ★ R1656 — the card's parts are its CHILDREN, not its siblings.
///
/// They were siblings until a person reported text outside the border and
/// nothing here could see it. §5.15's containment read judges a mark against
/// its parent, `scene/text_painted` names a run's nearest tagged ancestor, and
/// the smear gate groups by that ancestor — so with the parts flattened into
/// the canvas's child list, every one of those three questions was being asked
/// about the CANVAS, which is big enough to hold anything. The scene was
/// describing a screen that did not exist (§2 #7), and the checks were honest
/// answers to the wrong question.
///
/// Nesting costs the card-local coordinates below: an absolutely-positioned
/// child is placed relative to its parent (R1648 measured what forgetting that
/// looks like — every mark at twice its offset), which is why `CardShape`
/// hands out local rectangles for the parts and a window rectangle for the card.
fn canvas_cards(state: &LabState, ink: Ink) -> Vec<Scene> {
    let mut children: Vec<Scene> = Vec::new();
    let selected = state.selected.get();
    for node in state.cards() {
        let Some(shape) = card_shape(state, node) else {
            continue;
        };
        let name = state.name_of(node);
        let rows = card_rows(state, node);
        let role = state.role_of(node).unwrap_or(Role::Peer);
        let chosen = selected == Some(node);
        let mut parts: Vec<Scene> = Vec::new();
        parts.push(tagged_label(
            &format!("lab.node.{name}.id"),
            name.clone(),
            shape.id,
            shape.id_font,
            ink.text,
        ));
        parts.push(box_at(
            &format!("lab.node.{name}.badge"),
            shape.badge,
            ink.surface,
            Some(role_ink(role)),
            4,
        ));
        parts.push(label(
            role.badge(),
            shape.badge_text,
            shape.badge_font,
            role_ink(role),
        ));
        for ((key, value), (key_rect, value_rect)) in rows.iter().zip(shape.rows.iter()) {
            parts.push(label(key.clone(), *key_rect, shape.row_font, ink.text_3));
            // The value column holds user data — an endpoint, a key
            // expression — and its TAIL is what distinguishes one from another,
            // so the middle gives way rather than the end.
            parts.push(value_label(
                value.clone(),
                *value_rect,
                shape.row_font,
                ink.text_2,
            ));
        }
        let mut style = BoxStyle::filled(ink.surface).with_corner_radius(9);
        style = style.with_border(Border::new(
            if chosen { ink.accent } else { ink.outline_2 },
            1,
        ));
        children.push(Scene::Container(
            ContainerNode::new(parts)
                .with_tag(format!("lab.node.{name}"))
                .with_style(style)
                .with_layout(absolute(shape.rect)),
        ));
        children.extend(canvas_pins(state, node, shape.rect, role, ink));
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
    // ★ R1678 — one button per scope that has something to put back, from the
    // SAME list the hit test resolves against.
    for (scope, seat) in reset_seats(state) {
        let seat = local(seat);
        children.push(box_at(
            &format!("lab.reset.{}", scope.wire()),
            seat,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        children.push(label(
            scope.wire(),
            Rect::new(seat.x + 7, seat.y + 5, seat.w.saturating_sub(14), 12),
            FONT_SMALL,
            ink.text_2,
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
    let (world_w, world_h) = world_extent(state);
    let world = Scene::Container(
        ContainerNode::new(canvas_world(state, ink))
            .with_style(BoxStyle::filled(ink.bg))
            .with_layout(LayoutStyle::new().with_size(Size::px(world_w, world_h))),
    );
    let (ox, oy) = world_offset(state, state.pan.get());
    let viewport = Scene::Scroll(
        ScrollNode::new(panel_content(rect), world)
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

fn inspector(state: &LabState, field: (TextFieldState, u32), theme: &Theme, ink: Ink) -> Scene {
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
            vec![scroll_pane(
                &state.inspector_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )],
        );
    };
    children.extend(inspector_identity(state, node, ink));
    children.extend(inspector_edit(state, field, theme, ink));
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
    let mut parts = vec![
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
    ];
    // ★★ R1682 — the node's-life row. Painted for a selected card only, which
    // is the same condition the hit test asks: an act on "the selected card"
    // with no card selected is a button that cannot mean anything.
    let (collapsed, disabled) = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .map_or((false, false), |slot| {
            (slot.appearance.collapsed, slot.disabled)
        });
    for act in NodeAct::ALL {
        let seat = act.local_seat();
        // Delete is the one that cannot be undone, so it is the one drawn in
        // the warning ink — the reference does the same, and a row of three
        // identical buttons where the third destroys work is a row that
        // invites the wrong press.
        let (fill, edge, text) = match act {
            NodeAct::Delete => (ink.surface, ink.warn, ink.warn),
            _ if act == NodeAct::Collapse && collapsed || act == NodeAct::Disable && disabled => {
                (ink.accent_soft, ink.accent_line, ink.accent)
            }
            _ => (ink.raised, ink.outline, ink.text_2),
        };
        parts.push(box_at(act.tag(), seat, fill, Some(edge), 6));
        parts.push(label(
            act.word(collapsed, disabled),
            Rect::new(seat.x + 8, seat.y + 6, seat.w.saturating_sub(12), 13),
            FONT_SMALL,
            text,
        ));
    }
    parts
}

/// ★★★ R1683 — the one text field, and the seat that opens it on the name.
///
/// The field is the framework's own (`TextEditState` + the text-field painter),
/// so this screen gets the caret, the selection, the clipboard, the undo stack
/// and the IME composition path without writing any of them — and gets them the
/// same way the sibling node editor does, which is the fourth call site of the
/// lifted edit keymap rather than a fifth implementation of one.
fn inspector_edit(
    state: &LabState,
    field: (TextFieldState, u32),
    theme: &Theme,
    ink: Ink,
) -> Vec<Scene> {
    let (box_rect, seat, key_seat) = rename_row();
    let editing = state.editing.get();
    let mut parts = Vec::new();
    if editing.is_some() {
        let style = tf_paint::TextFieldStyle {
            field_w: box_rect.w,
            field_h: box_rect.h,
            field_pad: 5,
            font_size_px: FONT_SMALL + 1,
            ..tf_paint::TextFieldStyle::m3_filled()
        };
        parts.push(Scene::Container(
            ContainerNode::new(vec![tf_paint::view_field(
                EDIT_TAG, field.0, field.1, theme, &style, "name",
            )])
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(box_rect.x, box_rect.y)
                    .with_size(Size::px(box_rect.w, box_rect.h)),
            ),
        ));
    } else {
        parts.push(box_at(
            "lab.inspector.name",
            box_rect,
            ink.raised,
            Some(ink.outline),
            6,
        ));
        parts.push(label(
            "type a name or a key",
            Rect::new(
                box_rect.x + 8,
                box_rect.y + 6,
                box_rect.w.saturating_sub(12),
                13,
            ),
            FONT_SMALL,
            ink.text_3,
        ));
    }
    let word = if editing.is_some() { "apply" } else { "rename" };
    parts.push(box_at(
        "lab.inspector.rename",
        seat,
        ink.accent_soft,
        Some(ink.accent_line),
        6,
    ));
    parts.push(label(
        word,
        Rect::new(seat.x + 8, seat.y + 6, seat.w.saturating_sub(12), 13),
        FONT_SMALL,
        ink.accent,
    ));
    // The second target. A seat of its own rather than a mode on the first,
    // because "rename this card" and "give it a configuration path it does not
    // have" are different requests and a person should not have to know that
    // one button means both.
    parts.push(box_at(
        "lab.inspector.addkey",
        key_seat,
        ink.raised,
        Some(ink.outline),
        6,
    ));
    parts.push(label(
        "+ key",
        Rect::new(
            key_seat.x + 8,
            key_seat.y + 6,
            key_seat.w.saturating_sub(12),
            13,
        ),
        FONT_SMALL,
        ink.text_2,
    ));
    parts
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
    // ★ R1662 — the PANE-LOCAL geometry: the form now lives inside the
    // inspector's scrolling body, so it is placed in the body's frame and the
    // window-coordinate consumers derive theirs from it.
    let geometry = inspector_geometry_local(state);
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

    children.push(box_at(
        "lab.inspector.note",
        Rect::new(PAD, note_y, INSP_W - PAD * 2, 40),
        ink.raised,
        Some(ink.warn),
        8,
    ));
    children.push(tagged_label(
        "lab.inspector.note.text",
        restart_note,
        Rect::new(PAD + 10, note_y + 8, INSP_W - PAD * 2 - 20, 26),
        10,
        ink.text_2,
    ));
    // ★ R1662 — the form is now a CHILD of the pane body rather than a sibling
    // of the pane. It was a sibling because its geometry was in window
    // coordinates and nesting it under an absolutely-placed pane offset it
    // twice (R1648); with the geometry stated in the body's own frame that
    // reason is gone, and being a sibling is what kept the form from
    // scrolling. A list field has no bounded length, so no fixed pane floor is
    // enough — measured at R1652.1, a six-element list puts two chips and the
    // note below the window ([[debt-the-node-lab-panes-do-not-scroll]]).
    children.push(painted);
    panel(
        "lab.inspector",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![scroll_pane(
            &state.inspector_scroll,
            panel_content(rect),
            (0, PAD),
            // Every press on this screen belongs to the one root `External`
            // that does the screen's own hit test, so the pane must be
            // invisible to the router (R1655).
            PanePointer::PassesThrough,
            children,
        )],
    )
}

fn view(field: (TextFieldState, u32), _frame: Frame) -> Scene {
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
            inspector(&state, field, &theme, ink),
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
    /// R1656 §5.15 — the size the shell says this widget currently has, kept
    /// because `External::pointer_move` hands a FRACTION of it and not the
    /// rectangle itself. Seeded with the opening size and replaced by every
    /// [`External::on_resize`].
    surface: (u32, u32),
}

impl core::fmt::Debug for LabOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LabOracle").finish_non_exhaustive()
    }
}

impl LabOracle {
    const fn new() -> Self {
        Self {
            state: None,
            surface: (WIN_W, WIN_H),
        }
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

    /// The drawn link a caller named, refusing one that is not there (R1681).
    ///
    /// One parser, so `select_link`, `delete_link` and `relink` cannot disagree
    /// about what a link is called on the wire.
    ///
    /// ★ Two spellings, and the second is the one that matters: a link id is
    /// **minted in seeding order**, so an argument written as `3` is a caller
    /// asserting something about the order this screen happened to author its
    /// opening graph in. `P-01>R-01` names the same link by what it *is*. The
    /// pair form resolves against drawn links first and reported ones second,
    /// which is the order the canvas's own hit test uses.
    fn link_id(state: &LabState, raw: &str) -> Result<LinkId, InvokeError> {
        if let Some((from, to)) = raw.trim().split_once('>') {
            let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim())) else {
                return Err(InvokeError::rejected(format!(
                    "{:?} or {:?} is not a node on the canvas",
                    from.trim(),
                    to.trim()
                )));
            };
            return state
                .doc
                .borrow()
                .tree(ROOT)
                .and_then(|t| {
                    t.links()
                        .iter()
                        .find(|l| l.from.node == a && l.to.node == b)
                        .map(|l| l.id)
                })
                .ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "no link is drawn from {} to {}",
                        from.trim(),
                        to.trim()
                    ))
                });
        }
        let id: u32 = raw
            .trim()
            .parse()
            .map_err(|_| InvokeError::rejected(format!("{raw:?} is not a link id")))?;
        state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|t| t.link(LinkId(id)).map(|_| LinkId(id)))
            .ok_or_else(|| InvokeError::rejected(format!("no link {id} is drawn")))
    }

    /// The card a caller named, refusing one that is not on the canvas
    /// (R1682).
    ///
    /// One parser, so the four verbs of a node's life cannot disagree about
    /// what a card is called or how a wrong name is refused — the same reason
    /// [`Self::link_id`] exists next door.
    fn card(state: &LabState, name: &str) -> Result<NodeId, InvokeError> {
        state
            .node_of(name)
            .ok_or_else(|| InvokeError::rejected(format!("no node is called {name:?}")))
    }

    /// A link on either layer, in the spelling `selected_link` reads back.
    fn link_pick(state: &LabState, raw: &str) -> Result<LinkPick, InvokeError> {
        if let Ok(drawn) = Self::link_id(state, raw) {
            return Ok(LinkPick::Authored(drawn));
        }
        let Some((from, to)) = raw.split_once('>') else {
            return Self::link_id(state, raw).map(LinkPick::Authored);
        };
        let (Some(a), Some(b)) = (state.node_of(from.trim()), state.node_of(to.trim())) else {
            return Err(InvokeError::rejected(format!(
                "{:?} or {:?} is not a node on the canvas",
                from.trim(),
                to.trim()
            )));
        };
        state
            .doc
            .borrow()
            .observations(ROOT)
            .into_iter()
            .find(|o| o.from.node == a && o.to.node == b)
            .map(|o| LinkPick::Observed(o.from, o.to))
            .ok_or_else(|| {
                InvokeError::rejected(format!(
                    "nothing was reported from {} to {}",
                    from.trim(),
                    to.trim()
                ))
            })
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
        // ★ R1677 — declared, because since R1637 the declaration is a
        // PRECONDITION of dispatch and not a description of it: an arm added to
        // `query` without a line here answers `UnknownIntrospectPath`, which is
        // what these two did on their first drive.
        SchemaField::new("layout", "string"),
        // R1682 — the node's-life switches, per card.
        SchemaField::new("cards", "string"),
        // R1683 — what the one text field is doing, and what it holds.
        SchemaField::new("editing", "string"),
        SchemaField::new("frames", "string"),
        SchemaField::new("changed", "string"),
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
        // R1678 — the scope vocabulary is published, so an agent reads the five
        // rather than discovering them by rejection.
        SchemaField::action_with(
            "reset",
            "string",
            ArgForm::Scalar,
            const {
                &[SchemaArg::one_of(
                    "scope",
                    "string",
                    &ResetScope::WIRE_NAMES,
                )]
            },
        ),
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
        // ★★ R1682 — a node's own life. Three take just the card's name and
        // one takes the new name beside it, so only that one declares a
        // grammar; `collapse` and `disable` are toggles and answer the state
        // they left the card in.
        SchemaField::action("delete_node", "string"),
        SchemaField::action_with(
            "rename",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("node", "string", "nodes"),
                    SchemaArg::open("name", "string"),
                ]
            },
        ),
        SchemaField::action("collapse", "string"),
        SchemaField::action("disable", "string"),
        // ★★ R1683 — the one text field's own verbs. `edit` declares the two
        // things this screen types into, so an agent reads the vocabulary
        // rather than discovering it by rejection.
        SchemaField::action_with(
            "edit",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::one_of("target", "string", &["name", "key"])] },
        ),
        SchemaField::action("type", "string"),
        SchemaField::action("apply", "string"),
        SchemaField::action("add_key", "string"),
        // ★★ R1681 — the other half of a link's life. Four verbs and not one
        // with a mode, because they take different arguments and answer
        // different refusals: what a caller has to say to delete a link and
        // what it has to say to re-aim one are not the same sentence.
        SchemaField::action("delete_link", "string"),
        SchemaField::action_with(
            "relink",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("link", "string", "links"),
                    SchemaArg::key("to", "string", "nodes"),
                ]
            },
        ),
        SchemaField::action("set_endpoint", "string"),
        SchemaField::action_with(
            "adopt",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("from", "string", "nodes"),
                    SchemaArg::key("to", "string", "nodes"),
                ]
            },
        ),
        SchemaField::new("observed", "string"),
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
    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("the lab holds no document yet"))?;
        let text = |s: String| Ok(IntrospectValue::Text(s));
        match path {
            "spec" => text(spec_json().to_string()),
            "graph" => text(spec::GRAPH_NAME.to_owned()),
            "selected" => text(state.selected.get().map(|n| state.name_of(n)).unwrap_or_default()),
            // ★ R1681 — a drawn link answers its id and a reported one answers
            // the pair it runs between, which is the only name it has. The two
            // spellings are told apart by the `>`, and `select_link` admits
            // both, so what this reads back is what that takes.
            "selected_link" => text(
                state
                    .selected_link
                    .get()
                    .map(|pick| match pick {
                        LinkPick::Authored(id) => id.0.to_string(),
                        LinkPick::Observed(from, to) => format!(
                            "{}>{}",
                            state.name_of(from.node),
                            state.name_of(to.node)
                        ),
                    })
                    .unwrap_or_default(),
            ),
            "zoom" => Ok(IntrospectValue::Int(i64::from(state.zoom.get()))),
            "pan" => {
                let (x, y) = state.pan.get();
                text(format!("{x},{y}"))
            }
            "running" => Ok(IntrospectValue::Bool(state.running.get())),
            "discovery" => Ok(IntrospectValue::Bool(state.discovery.get())),
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
                let form = selected_form(state)
                .ok_or_else(|| ReadRefusal::unavailable("no node is selected"))?;
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
                let form = selected_form(state)
                .ok_or_else(|| ReadRefusal::unavailable("no node is selected"))?;
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
                let tree = doc
                .tree(ROOT)
                .ok_or_else(|| ReadRefusal::no_such_member("the document has no root tree"))?;
                text(
                    serde_json::Value::Array(
                        tree.links()
                            .iter()
                            .map(|l| {
                                serde_json::json!({
                                    "id": l.id.0,
                                    "from": state.name_of(l.from.node),
                                    "to": state.name_of(l.to.node),
                                    // ★ R1681 — WHICH endpoint of the target
                                    // this link dialled. Published because it
                                    // is what the endpoint seats move and an
                                    // operation whose result cannot be read is
                                    // indistinguishable from one that did
                                    // nothing.
                                    "endpoint": endpoint_at(state, l.to).unwrap_or_default(),
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            // ★★ R1681 — the other layer: what a source reported. Its own slot
            // rather than a flag on `links`, because it is not in the graph and
            // a reader that had to filter a mixed list would be one `if` away
            // from treating a claim about the world as a drawing.
            "observed" => {
                let doc = state.doc.borrow();
                text(
                    serde_json::Value::Array(
                        doc.observations(ROOT)
                            .into_iter()
                            .map(|seen| {
                                serde_json::json!({
                                    "from": state.name_of(seen.from.node),
                                    "to": state.name_of(seen.to.node),
                                    "endpoint": endpoint_of(&doc, seen.to).unwrap_or_default(),
                                    "layer": doc
                                        .link_layer(ROOT, seen.from, seen.to)
                                        .map(LinkLayer::name)
                                        .unwrap_or_default(),
                                })
                            })
                            .collect(),
                    )
                    .to_string(),
                )
            }
            // ★★ R1678 — which scopes differ from what the screen opened as.
            //
            // Published because the affordances are DERIVED from it: a reset
            // button exists exactly when its scope is here, so a driver that
            // read the screen and a person looking at it are reading one fact.
            // The reference publishes the same predicates to its own view for
            // the same reason (measured — three of its four gated resets are
            // wrapped in a conditional on one of these).
            "changed" => text(
                serde_json::Value::Object(
                    ResetScope::ALL
                        .into_iter()
                        .map(|scope| {
                            (
                                scope.wire().to_owned(),
                                serde_json::Value::Bool(scope.changed(state)),
                            )
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★★ R1677 — WHERE the cards are, which nothing published until the
            // operation gate asked for it. Three of the reference's operations
            // move a node or a frame, and an agent driving any of them could
            // not observe its own result: the canvas positions existed only
            // inside the document and reached the wire nowhere. A gesture whose
            // effect cannot be read is one no test can distinguish from a
            // gesture that did nothing, which is exactly how "the frame drags
            // but does not select" survived.
            //
            // Canvas coordinates, not window ones: this is where a node sits in
            // the GRAPH, which is what a caller placing or comparing nodes
            // means. `scene/tag_rects` answers the window question for the same
            // cards, and the two are deliberately different reads.
            "layout" => text(
                serde_json::Value::Object(
                    state
                        .cards()
                        .into_iter()
                        .filter_map(|node| {
                            let doc = state.doc.borrow();
                            let slot = doc.tree(ROOT)?.node(node)?;
                            Some((
                                state.name_of(node),
                                serde_json::json!([slot.x, slot.y]),
                            ))
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★★ R1682 — the two switches a card carries: whether it is drawn
            // small, and whether it runs at all.
            //
            // Read together because the affordance shows them together — they
            // are the node's-life row, one press each — and published for the
            // R1677 reason: neither moves a name or a position, so without a
            // slot of their own an agent could collapse a card and have no way
            // to observe that it had. A gesture whose effect cannot be read is
            // one no test can tell from a gesture that did nothing.
            //
            // ★ They are two different KINDS of fact and the wire says so by
            // keeping them apart rather than folding them into one "state"
            // word: `collapsed` is a look, and the model keeps it with the
            // node's appearance; `disabled` is what the graph MEANS, and the
            // model keeps it beside the node's body. A reader that wanted only
            // one of them would otherwise have to know which half of a blended
            // answer to trust.
            "cards" => text(
                serde_json::Value::Object(
                    state
                        .cards()
                        .into_iter()
                        .filter_map(|node| {
                            let doc = state.doc.borrow();
                            let slot = doc.tree(ROOT)?.node(node)?;
                            Some((
                                slot.display_name(),
                                serde_json::json!({
                                    "collapsed": slot.appearance.collapsed,
                                    "disabled": slot.disabled,
                                }),
                            ))
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★ R1677 — which host each card starts on. The membership a drop
            // changes, and the other half of the same silence: `apply_frame`
            // re-parents a node and the only witness was a toast sentence.
            "frames" => text(
                serde_json::Value::Object(
                    state
                        .cards()
                        .into_iter()
                        .map(|node| {
                            let frame = state
                                .doc
                                .borrow()
                                .tree(ROOT)
                                .and_then(|t| t.node(node))
                                .and_then(|slot| slot.parent)
                                .and_then(|f| state.frames.borrow().get(&f).cloned());
                            (
                                state.name_of(node),
                                frame.map_or(serde_json::Value::Null, serde_json::Value::String),
                            )
                        })
                        .collect(),
                )
                .to_string(),
            ),
            // ★ R1683 — what the one field is doing, and what it holds. Both,
            // because "the editor is open" and "it says this" are different
            // facts and a driver checking its own typing needs the second.
            "editing" => text(
                serde_json::json!({
                    "target": state.editing.get().map(Editing::wire),
                    "text": state.buffer.text(),
                })
                .to_string(),
            ),
            "roles" => text(
                Role::ALL
                    .into_iter()
                    .map(|r| format!("{}:{}", r.group(), r.name()))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "toast" => text(state.toast.get()),
            _ => Err(ReadRefusal::UnknownPath),
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
            // ★★ R1682 — the node's own life. Four verbs over one argument,
            // the card's name, which is the name the canvas shows and the wire
            // reads back.
            "delete_node" => {
                let name = Self::text(&args)?;
                let node = Self::card(&state, name.trim())?;
                delete_card(&state, node).map(IntrospectValue::Text)
            }
            "rename" => {
                let raw = Self::text(&args)?;
                let (which, to) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <node>,<name>"))
                })?;
                let node = Self::card(&state, which.trim())?;
                rename_card(&state, node, to.trim()).map(IntrospectValue::Text)
            }
            // ★★ R1683 — the field's own three verbs. `edit` opens it on a
            // target, `type` puts text in it, `apply` does the thing. Three and
            // not one, because an agent that could only "rename with this
            // string" could never exercise the path a PERSON takes — which is
            // exactly the column every defect on this screen has lived in.
            "edit" => {
                let what = Self::text(&args)?;
                let node = state
                    .selected
                    .get()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                let target = match what.trim() {
                    "name" => Editing::Name(node),
                    "key" => Editing::Key(node),
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a thing this screen edits"
                        )));
                    }
                };
                Ok(IntrospectValue::Text(begin_edit(&state, target)))
            }
            "type" => {
                let text = Self::text(&args)?;
                if state.editing.get().is_none() {
                    return Err(InvokeError::rejected("nothing is being edited"));
                }
                state.buffer.set_text(text.clone());
                Ok(IntrospectValue::Text(text))
            }
            "apply" => commit_edit(&state).map(IntrospectValue::Text),
            // ★ The one-shot, beside the box's three — the same arrangement
            // `rename` has. An agent that knows the key it wants says so; the
            // box is for a person who is finding out.
            "add_key" => {
                let key = Self::text(&args)?;
                let node = state
                    .selected
                    .get()
                    .ok_or_else(|| InvokeError::rejected("no node is selected"))?;
                add_key(&state, node, key.trim()).map(IntrospectValue::Text)
            }
            "collapse" => {
                let name = Self::text(&args)?;
                let node = Self::card(&state, name.trim())?;
                collapse_card(&state, node).map(IntrospectValue::Text)
            }
            "disable" => {
                let name = Self::text(&args)?;
                let node = Self::card(&state, name.trim())?;
                disable_card(&state, node).map(IntrospectValue::Text)
            }
            // ★ R1681 — either layer, told apart by the `>`. A reported link
            // has no id to name it by, so the pair is the name; refusing to let
            // one be picked at all would have made the adopt affordance
            // unreachable to everything but a pointer.
            "select_link" => {
                let raw = Self::text(&args)?;
                let pick = Self::link_pick(&state, raw.trim())?;
                state.selected_link.set(Some(pick));
                Ok(IntrospectValue::Text(raw.trim().to_owned()))
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
            // ★★ R1678 — one action with a scope argument, not five actions.
            // The scopes are a closed set the specification already names, and
            // five verbs would be five places for that set to drift; the
            // declaration below publishes the options, so an agent discovers
            // them rather than guessing.
            "reset" => {
                let word = Self::text(&args)?;
                let scope = ResetScope::ALL
                    .into_iter()
                    .find(|s| s.wire() == word.trim())
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "{:?} is not a scope; they are {}",
                            word.trim(),
                            ResetScope::ALL.map(ResetScope::wire).join(" / ")
                        ))
                    })?;
                scope.apply(&state);
                state.say(format!("{} back to how it opened", scope.wire()));
                Ok(IntrospectValue::Text(scope.wire().to_owned()))
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
            // ★★ R1681 — a link's life after it is drawn.
            "delete_link" => {
                let id = Self::link_id(&state, &Self::text(&args)?)?;
                delete_link(&state, id).map(IntrospectValue::Text)
            }
            "relink" => {
                let raw = Self::text(&args)?;
                let (link, to) = raw
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <link>,<to>")))?;
                let id = Self::link_id(&state, link)?;
                let node = state.node_of(to.trim()).ok_or_else(|| {
                    InvokeError::rejected(format!("{:?} is not a node on the canvas", to.trim()))
                })?;
                relink_to(&state, id, node).map(IntrospectValue::Text)
            }
            // A number, because the seats a person can press are a numbered row
            // and an agent addressing them by locator would be addressing a
            // different thing from the one on screen.
            "set_endpoint" => {
                let raw = Self::text(&args)?;
                let n: usize = raw.trim().parse().map_err(|_| {
                    InvokeError::rejected(format!("{raw:?} is not an endpoint number"))
                })?;
                choose_endpoint(&state, n).map(IntrospectValue::Text)
            }
            "adopt" => {
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
                let seen = state
                    .doc
                    .borrow()
                    .observations(ROOT)
                    .into_iter()
                    .find(|o| o.from.node == a && o.to.node == b)
                    .ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "nothing was reported from {} to {}",
                            from.trim(),
                            to.trim()
                        ))
                    })?;
                adopt_link(&state, seen.from, seen.to).map(IntrospectValue::Text)
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
        // ★ R1664 — `body` is published too. R1662 added the column to the
        // specification and to the Rust sweep and stopped there, so the pane
        // that scrolls painted a tag the WIRE's copy of the specification did
        // not declare, and the demo's backward check went red on CI while
        // every local test passed. A fact added to the model and published by
        // half is the shape this project keeps paying for.
        "panes": spec::PANES.iter().map(|p| serde_json::json!({
            "tag": p.tag, "title": p.title, "width": p.width, "body": p.body,
        })).collect::<Vec<_>>(),
        // ★ R1681 — published so the demo's family pin is derived from the
        // specification rather than written down, the same way `links` is.
        "observed": spec::OBSERVED.iter().map(|(from, to)| serde_json::json!({
            "from": from, "to": to,
        })).collect::<Vec<_>>(),
        "rail": spec::RAIL.iter().map(|(name, reserved_for)| serde_json::json!({
            "name": name,
            "locked": reserved_for.is_some(),
            "reserved_for": reserved_for,
            "active": *name == spec::RAIL_ACTIVE,
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
        // ★ R1678 — the reset affordances, and which of them are CONDITIONAL.
        // Published rather than left for a reader to infer from the operations
        // list, because the conditional ones are the reason a backward check
        // must accept a tag that is not always there — and R1664 is what
        // happens when a family reaches the paint tree and not this table.
        "resets": ResetScope::ALL.iter().map(|scope| serde_json::json!({
            "scope": scope.wire(),
            "gated": scope.gated(),
        })).collect::<Vec<_>>(),
        // ★★ R1677 — what the screen can be asked to DO, beside what it has.
        // Published for the same reason every other row here is: a demo that
        // carried its own copy of this list would be checking the list against
        // itself. `absent` is derived rather than stored, so an operation
        // cannot be declared missing and reachable at once.
        "operations": spec::OPERATIONS.iter().map(|op| serde_json::json!({
            "name": op.name,
            "verb": op.verb.map(|(verb, arg)| serde_json::json!([verb, arg])),
            "gesture": op.gesture,
            "witness": op.witness,
            "needs": op.needs,
            "absent": op.verb.is_none() && !op.gesture,
        })).collect::<Vec<_>>(),
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

/// The locators a node listens on, in the order its form lists them.
///
/// ★★ R1681 — the population behind every endpoint decision on this screen.
/// A node listening in two places can be dialled in two ways, and *which one a
/// link took* is a property of the link, not of the node: it is the first thing
/// the reference says about its own equivalent, and it is why an endpoint can
/// be re-chosen on a wire that is already drawn.
fn endpoints_in(forms: &BTreeMap<NodeId, ConfigForm>, node: NodeId) -> Vec<String> {
    forms
        .get(&node)
        .and_then(|form| form.field("listen.endpoints").map(|f| f.value().to_owned()))
        .map(|value| {
            FieldType::elements(&value)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn endpoints_of(state: &LabState, node: NodeId) -> Vec<String> {
    endpoints_in(&state.forms.borrow(), node)
}

/// Which endpoint the link landing on `socket` dialled.
///
/// The accept run carries one item per link that lands on the node, and the
/// item's **label is the endpoint**. That is the whole of the endpoint model:
/// one fact, in the place the model already keeps per-slot facts, so nothing
/// maintains a parallel table of which wire took which address.
fn endpoint_of(doc: &Document<LabNode>, socket: Socket) -> Option<String> {
    doc.items(ROOT, socket.node, Side::Input)?
        .get(socket.port as usize)
        .and_then(|item| item.label.clone())
}

fn endpoint_at(state: &LabState, socket: Socket) -> Option<String> {
    endpoint_of(&state.doc.borrow(), socket)
}

/// Make an accept slot on `to` that dials `endpoint`, and answer its port.
///
/// Always a fresh slot: an accept endpoint is a listening socket and is dialled
/// by as many peers as reach it, while the model's value input takes one
/// producer, so the run is how the many-ness is expressed and one item per
/// arriving link is what keeps the two consistent. The item is **typed** by the
/// endpoint's own transport, which is what makes a dial that cannot speak it a
/// refusal the model states rather than a defect the gate notices later.
/// `endpoint` is `None` for a node that can be dialled and has **nowhere to
/// listen** — a real state on this screen, and one the launch gate already
/// names rather than one the canvas should refuse to draw. The slot is then
/// unlabelled, which is exactly true: the link dials no address.
fn open_slot_in(doc: &mut Document<LabNode>, to: NodeId, endpoint: Option<&str>) -> Option<u32> {
    let arity = u32::try_from(doc.signature(ROOT, to)?.inputs.len()).unwrap_or(0);
    let item = match endpoint {
        Some(one) => Item::plain()
            .named(one)
            .typed(0, Transport::of_locator(one).unwrap_or(Transport::Tcp)),
        None => Item::plain(),
    };
    doc.insert_item(ROOT, to, Side::Input, arity, item).ok()?;
    let mut port = arity;
    // ★ The run declares `at_least(1)`, so a node that has never been dialled
    // still carries one slot — and appending beside it would leave every
    // accepting node with a dead port forever. Measured: the opening graph had
    // one per accepting node, found by a census that counts BOTH directions.
    // Dropped here rather than never created, because the floor is the crate's
    // and this is the first moment a real slot exists to replace it.
    if let Some(dead) = spare_slot(doc, to, port) {
        if doc.remove_item(ROOT, to, Side::Input, dead).is_ok() && dead < port {
            port -= 1;
        }
    }
    Some(port)
}

/// A slot before `keep` that names no address and holds nothing (R1681).
fn spare_slot(doc: &Document<LabNode>, node: NodeId, keep: u32) -> Option<u32> {
    let items = doc.items(ROOT, node, Side::Input)?;
    (0..keep).find(|port| {
        let empty = items
            .get(*port as usize)
            .is_none_or(|item| item.label.is_none());
        let socket = Socket::new(node, *port);
        empty
            && !doc
                .tree(ROOT)
                .is_some_and(|t| t.links().iter().any(|l| l.to == socket))
            && !doc.observations(ROOT).iter().any(|o| o.to == socket)
    })
}

fn open_slot(state: &LabState, to: NodeId, endpoint: Option<&str>) -> Option<u32> {
    open_slot_in(&mut state.doc.borrow_mut(), to, endpoint)
}

/// The endpoint a new link from `from` would dial on `to`, or why it cannot be
/// made (R1681).
///
/// Three answers and not two: an endpoint, **no endpoint at all** for a node
/// that listens nowhere (which is drawable and is what the launch gate is for),
/// and a refusal when this dialler has already taken every address the target
/// offers — which is the reference's rule and is about the *pair*, since two
/// different peers may of course dial the same address.
fn landing_endpoint(
    doc: &Document<LabNode>,
    forms: &BTreeMap<NodeId, ConfigForm>,
    from: NodeId,
    to: NodeId,
) -> Result<Option<String>, ()> {
    if endpoints_in(forms, to).is_empty() {
        return Ok(None);
    }
    free_endpoints_in(doc, forms, from, to)
        .into_iter()
        .next()
        .map_or(Err(()), |one| Ok(Some(one)))
}

/// Drop the accept slot at `port`, now that nothing lands on it.
///
/// The run would otherwise only ever grow: every link that arrives opens a
/// slot, so every link that leaves has to close one. `remove_item` re-points
/// the links past it, which is the reason this is one crate call and not an
/// index fixup here.
fn close_slot(state: &LabState, node: NodeId, port: u32) {
    let still_used = state
        .doc
        .borrow()
        .tree(ROOT)
        .is_some_and(|t| t.links().iter().any(|l| l.to == Socket::new(node, port)))
        || state
            .doc
            .borrow()
            .observations(ROOT)
            .iter()
            .any(|o| o.to == Socket::new(node, port));
    if still_used {
        return;
    }
    state
        .doc
        .borrow_mut()
        .remove_item(ROOT, node, Side::Input, port)
        .ok();
}

/// Which endpoints of `to` this dialler has not already taken.
///
/// The reference's rule, and it is about the **pair**: a second wire between
/// the same two nodes has to dial a different address, because that is what a
/// second transport connection is, while two *different* peers may of course
/// dial the same one.
fn free_endpoints_in(
    doc: &Document<LabNode>,
    forms: &BTreeMap<NodeId, ConfigForm>,
    from: NodeId,
    to: NodeId,
) -> Vec<String> {
    // Reported links hold an endpoint too: the world took it, so a drawing
    // that claimed the same one would be describing a connection that is not
    // the one out there.
    let landed = doc
        .tree(ROOT)
        .map(|t| {
            t.links()
                .iter()
                .filter(|l| l.from.node == from && l.to.node == to)
                .map(|l| l.to)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let reported = doc
        .observations(ROOT)
        .into_iter()
        .filter(|o| o.from.node == from && o.to.node == to)
        .map(|o| o.to);
    let used: Vec<String> = landed
        .into_iter()
        .chain(reported)
        .filter_map(|socket| endpoint_of(doc, socket))
        .collect();
    endpoints_in(forms, to)
        .into_iter()
        .filter(|one| !used.contains(one))
        .collect()
}

/// Author a link, letting the crate refuse it.
fn connect(state: &Rc<LabState>, from: NodeId, to: NodeId) -> Result<String, InvokeError> {
    let name = state.name_of(to);
    let Ok(endpoint) = landing_endpoint(&state.doc.borrow(), &state.forms.borrow(), from, to)
    else {
        let said = format!(
            "{} already dials every endpoint of {name}",
            state.name_of(from)
        );
        state.say(said.clone());
        return Err(InvokeError::rejected(said));
    };
    let Some(port) = open_slot(state, to, endpoint.as_deref()) else {
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
            state.selected_link.set(Some(LinkPick::Authored(made.link)));
            let word = format!("{} -> {}", state.name_of(from), state.name_of(to));
            match &endpoint {
                Some(one) => state.say(format!("linked {word} on {one}")),
                None => state.say(format!("linked {word}")),
            }
            Ok(word)
        }
        Err(why) => {
            // The slot was opened for a link that did not arrive.
            close_slot(state, to, port);
            let sentence = format!("{why:?}");
            state.say(format!("refused: {sentence}"));
            Err(InvokeError::rejected(sentence))
        }
    }
}

/// Remove a link somebody drew, and close the slot it was landing on (R1681).
fn delete_link(state: &Rc<LabState>, link: LinkId) -> Result<String, InvokeError> {
    let gone = state.doc.borrow_mut().disconnect(ROOT, link);
    match gone {
        Ok(gone) => {
            close_slot(state, gone.to.node, gone.to.port);
            if state.selected_link.get() == Some(LinkPick::Authored(link)) {
                state.selected_link.set(None);
            }
            let word = format!(
                "{} -> {}",
                state.name_of(gone.from.node),
                state.name_of(gone.to.node)
            );
            state.say(format!("unlinked {word}"));
            Ok(word)
        }
        Err(why) => {
            let sentence = format!("{why:?}");
            state.say(format!("refused: {sentence}"));
            Err(InvokeError::rejected(sentence))
        }
    }
}

// ── A node's life ───────────────────────────────────────────────────────────

/// Take a card off the canvas, with everything that hung on it (R1682).
///
/// ★ **The last card cannot go.** The reference refuses the same way and for
/// the same reason: a graph editor with an empty canvas has no selection, so
/// the inspector, the gate panel and every affordance keyed to a selected node
/// vanish at once — a state a person reaches by pressing delete one time too
/// many and cannot leave.
fn delete_card(state: &Rc<LabState>, node: NodeId) -> Result<String, InvokeError> {
    let name = state.name_of(node);
    if state.cards().len() <= 1 {
        let said = format!("{name} is the last card, so it stays");
        state.say(said.clone());
        return Err(InvokeError::rejected(said));
    }
    // The document answers what the removal took with it, which is the half of
    // the edit that is not where the gesture happened.
    let taken = state
        .doc
        .borrow_mut()
        .remove_node(ROOT, node)
        .map_err(|why| InvokeError::rejected(why.to_string()))?;
    // Every accept run this card was dialling keeps a slot per link, so the
    // links it took with it have to give their seats back — the same close the
    // link deletion does, for the same reason (R1681.1).
    for link in &taken.links {
        if link.to.node != node {
            close_slot(state, link.to.node, link.to.port);
        }
    }
    state.forms.borrow_mut().remove(&node);
    state.opened_at.borrow_mut().remove(&node);
    if state.selected.get() == Some(node) {
        state.selected.set(state.cards().first().copied());
    }
    // A picked link that ran through this card is a name for something that is
    // no longer there.
    let dangling = match state.selected_link.get() {
        Some(LinkPick::Authored(id)) => taken.links.iter().any(|l| l.id == id),
        Some(LinkPick::Observed(from, to)) => from.node == node || to.node == node,
        None => false,
    };
    if dangling {
        state.selected_link.set(None);
    }
    state.say(format!("deleted {name}, and {} link(s)", taken.links.len()));
    Ok(name)
}

// ── The one text field ──────────────────────────────────────────────────────

/// Open the shared field on `what`, seeded with the value it is about (R1683).
///
/// Seeded rather than blank, which is the reference's own choice for the name
/// box and the right one: a rename usually adjusts a name rather than replacing
/// it, and a person who wanted to replace it selects all. The key box opens
/// empty because there is no key yet.
/// Answers the seed rather than a `Result`: there is nothing here to refuse
/// once the buffer is the screen's own, which is what taking it at
/// construction bought.
fn begin_edit(state: &Rc<LabState>, what: Editing) -> String {
    let buffer = &state.buffer;
    let seed = match what {
        Editing::Name(node) => state.name_of(node),
        Editing::Key(_) => String::new(),
    };
    buffer.set_text(seed.clone());
    // Selected whole, so the first keystroke replaces rather than appends —
    // which is what a box that opens already holding a value has to do.
    buffer.set_selection(0, seed.len());
    state.editing.set(Some(what));
    pinion_core::focus_request::request(EDIT_TAG);
    state.say(format!("editing the {}", what.wire()));
    seed
}

/// What a press on one of the field's two seats does (R1683).
///
/// ★ One seat with two jobs and one with one, which is what makes the field's
/// state readable from the buttons: the name seat opens the box when it is shut
/// and applies what is in it when it is open, and the key seat always opens on
/// a path. The reference's own box works the same way.
fn edit_seat(state: &Rc<LabState>, which: &Hit) {
    let Some(node) = state.selected.get() else {
        return;
    };
    match which {
        Hit::AddKey => {
            begin_edit(state, Editing::Key(node));
        }
        _ if state.editing.get().is_some() => {
            commit_edit(state).ok();
        }
        _ => {
            begin_edit(state, Editing::Name(node));
        }
    }
}

/// Take what the field holds and do the thing it was opened for (R1683).
///
/// ★ The commit is the SAME verb the wire's own action calls, so a name typed
/// into the box and a name handed to `rename` cannot be refused differently.
/// A refusal leaves the field open with the text still in it, because a person
/// whose name was rejected wants to edit it, not to type it again.
fn commit_edit(state: &Rc<LabState>) -> Result<String, InvokeError> {
    let Some(what) = state.editing.get() else {
        return Err(InvokeError::rejected("nothing is being edited"));
    };
    let text = state.buffer.text();
    let done = match what {
        Editing::Name(node) => rename_card(state, node, text.trim()),
        Editing::Key(node) => add_key(state, node, text.trim()),
    };
    if done.is_ok() {
        end_edit(state);
    }
    done
}

/// Shut the field, leaving whatever it was editing alone.
fn end_edit(state: &Rc<LabState>) {
    state.editing.set(None);
    state.buffer.set_text(String::new());
    pinion_core::focus_request::request(VIEW_TAG);
}

/// Add a configuration path the catalogue does not offer (R1683).
///
/// ★★ The half of "add a field" the chips cannot do. The catalogue is a list of
/// the paths worth reaching for, not the boundary of what a configuration has —
/// the reference says exactly that beside its own key box — so any path the
/// form will accept can be typed. An already-held key is refused rather than
/// silently duplicated.
fn add_key(state: &Rc<LabState>, node: NodeId, key: &str) -> Result<String, InvokeError> {
    if key.is_empty() {
        return Err(InvokeError::rejected(
            "a key with nothing in it is not a key",
        ));
    }
    let mut forms = state.forms.borrow_mut();
    let form = forms
        .get_mut(&node)
        .ok_or_else(|| InvokeError::rejected("the card has no form"))?;
    // ★★ What a typed path IS — its type, its shape, whether it reaches a
    // running node — is this application's knowledge, not the widget's. A path
    // the catalogue already describes keeps that description; anything else
    // arrives as text that applies on restart, which is the safe reading of a
    // key nobody has classified.
    let described = spec::ADDABLE
        .iter()
        .find(|offered| **offered == key)
        .map_or_else(
            || ConfigField::new(key.to_owned(), "text", Applies::Restart, ""),
            |known| offered(known),
        );
    let outcome = form.add_typed(described);
    drop(forms);
    if let Err(why) = outcome {
        let said = why.to_string();
        state.say(said.clone());
        return Err(InvokeError::rejected(said));
    }
    sync_node(state, node);
    state.say(format!("added {key}"));
    Ok(key.to_owned())
}

/// Give a card a different name, keeping it the same card (R1682).
///
/// One verb for both callers — the rename action and the node reset putting a
/// name back — so "what a name is" has one definition. The refusal comes
/// straight from the model, which is the thing that knows whether a name is
/// already taken.
fn rename_card(state: &Rc<LabState>, node: NodeId, to: &str) -> Result<String, InvokeError> {
    let was = state.name_of(node);
    let done = state
        .doc
        .borrow_mut()
        .relabel(ROOT, node, Some(to))
        .map_err(|why| {
            let sentence = why.to_string();
            state.say(format!("refused: {sentence}"));
            InvokeError::rejected(sentence)
        })?;
    // ★ Nothing else to carry, and that is the measurement rather than an
    // omission: every other per-card record on this screen — the form, the
    // placement, the frame, the links — is keyed by the node's IDENTITY, which
    // a rename does not touch. The reference prototype has to move ten side
    // tables here because its rename remakes the node.
    if done.changed {
        state.say(format!("{was} -> {to}"));
    }
    Ok(to.to_owned())
}

/// Draw a card small, or full size again — a look, never a meaning (R1682).
fn collapse_card(state: &Rc<LabState>, node: NodeId) -> Result<String, InvokeError> {
    let now = {
        let mut doc = state.doc.borrow_mut();
        let slot = doc
            .tree_mut(ROOT)
            .and_then(|tree| tree.node_mut(node))
            .ok_or_else(|| InvokeError::rejected("no such card"))?;
        slot.appearance.collapsed = !slot.appearance.collapsed;
        slot.appearance.collapsed
    };
    let name = state.name_of(node);
    state.say(format!(
        "{name} {}",
        if now { "collapsed" } else { "expanded" }
    ));
    Ok(now.to_string())
}

/// Switch a card off, or back on (R1682).
///
/// ★★ The model's [`Document::set_disabled`], not `set_bypassed`: this screen's
/// nodes are processes, and switching one off means it does not run and nothing
/// downstream hears from it. Bypassing would mean the opposite — traffic routed
/// straight through — which is a request this tool never makes.
fn disable_card(state: &Rc<LabState>, node: NodeId) -> Result<String, InvokeError> {
    let was = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .map(|slot| slot.disabled)
        .ok_or_else(|| InvokeError::rejected("no such card"))?;
    state
        .doc
        .borrow_mut()
        .set_disabled(ROOT, node, !was)
        .map_err(|why| InvokeError::rejected(why.to_string()))?;
    let name = state.name_of(node);
    state.say(format!(
        "{name} {}",
        if was { "switched on" } else { "switched off" }
    ));
    Ok((!was).to_string())
}

/// Move a drawn link's consuming end onto `to`, dialling its first free
/// endpoint (R1681).
///
/// The link keeps its identity throughout — see `Document::relink`. What that
/// buys here is visible: the selection does not have to be repaired afterwards,
/// because the thing that was selected is the thing that moved.
fn relink_to(state: &Rc<LabState>, link: LinkId, to: NodeId) -> Result<String, InvokeError> {
    let held = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(link).copied())
        .ok_or_else(|| InvokeError::rejected(format!("no link {} is drawn", link.0)))?;
    let name = state.name_of(to);
    let Ok(endpoint) = landing_endpoint(
        &state.doc.borrow(),
        &state.forms.borrow(),
        held.from.node,
        to,
    ) else {
        let said = format!(
            "{} already dials every endpoint of {name}",
            state.name_of(held.from.node)
        );
        state.say(said.clone());
        return Err(InvokeError::rejected(said));
    };
    move_end(state, link, to, endpoint.as_deref()).map(|_| {
        let word = format!("{} -> {name}", state.name_of(held.from.node));
        state.say(format!("moved {word}"));
        word
    })
}

/// Take a reported link into the drawing (R1681).
///
/// `Document::adopt` runs the **authoring** rules on it, so a link the world
/// has and this model cannot express is *named* rather than quietly dropped.
/// That refusal is the finding the whole two-layer idea exists to produce, and
/// it is why this is not "copy the observation into the links list".
fn adopt_link(state: &Rc<LabState>, from: Socket, to: Socket) -> Result<String, InvokeError> {
    let taken = state.doc.borrow_mut().adopt(ROOT, from, to);
    match taken {
        Ok(made) => {
            state.selected_link.set(Some(LinkPick::Authored(made.link)));
            let word = format!("{} -> {}", state.name_of(from.node), state.name_of(to.node));
            state.say(format!("adopted {word}"));
            Ok(word)
        }
        Err(why) => {
            let sentence = format!("{why:?}");
            state.say(format!("refused: {sentence}"));
            Err(InvokeError::rejected(sentence))
        }
    }
}

/// Put the picked link on the target's `n`th listening endpoint (R1681).
///
/// The link decides which endpoint it dials — not the node — which is why this
/// moves the link's end rather than editing anything about the target. The
/// reference sets an index on the wire and checks nothing; here the endpoint's
/// own transport is the accept slot's type, so dialling one this link cannot
/// speak is refused by the model, with both transports named.
fn choose_endpoint(state: &Rc<LabState>, n: usize) -> Result<String, InvokeError> {
    let picked = state
        .selected_link
        .get()
        .and_then(LinkPick::authored)
        .ok_or_else(|| InvokeError::rejected("no drawn link is picked"))?;
    let to = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(picked).map(|l| l.to.node))
        .ok_or_else(|| InvokeError::rejected("the picked link is not drawn"))?;
    let endpoints = endpoints_of(state, to);
    let endpoint = endpoints.get(n).cloned().ok_or_else(|| {
        InvokeError::rejected(format!(
            "{} listens on {} endpoint(s), so there is no {n}",
            state.name_of(to),
            endpoints.len()
        ))
    })?;
    move_end(state, picked, to, Some(&endpoint)).map(|_| {
        state.say(format!("on {endpoint}"));
        endpoint
    })
}

/// The one arithmetic behind re-aiming a link and re-choosing its endpoint: a
/// slot is opened for where it is going, the crate moves the end, and whichever
/// slot is now empty is closed (R1681).
///
/// One function because the two operations differ only in whether the node on
/// the far end changes, and two copies of the open/move/close dance would be
/// two places for the run's bookkeeping to drift.
fn move_end(
    state: &Rc<LabState>,
    link: LinkId,
    to: NodeId,
    endpoint: Option<&str>,
) -> Result<Relinked, InvokeError> {
    let was = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|t| t.link(link).map(|l| l.to))
        .ok_or_else(|| InvokeError::rejected(format!("no link {} is drawn", link.0)))?;
    let Some(port) = open_slot(state, to, endpoint) else {
        return Err(InvokeError::rejected(format!(
            "{} has no accept pin",
            state.name_of(to)
        )));
    };
    let done = state
        .doc
        .borrow_mut()
        .relink(ROOT, link, Side::Input, Socket::new(to, port));
    match done {
        Ok(done) => {
            // The old slot last: closing it re-points what is past it, and the
            // link has already left it.
            close_slot(state, was.node, was.port);
            Ok(done)
        }
        Err(why) => {
            close_slot(state, to, port);
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
        // Both follow the cursor and neither changes the document until
        // release: what the canvas draws mid-drag comes from `cursor`.
        Drag::Wire { .. } | Drag::Rewire { .. } => {}
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
        // ★★ R1681 — pressing an accept pin that already holds a wire PICKS IT
        // UP, which is the reference's rule and every node editor's. A dial pin
        // is fan-out and always starts a new wire; an accept pin holds what
        // arrived, so grabbing it means "move this one".
        Hit::Pin { node, dial: false } => {
            let at = window_to_content(state, px, py);
            if let Some(link) = link_into_pin(state, *node, at) {
                let source = state
                    .doc
                    .borrow()
                    .tree(ROOT)
                    .and_then(|t| t.link(link).map(|l| l.from.node));
                if let Some(from) = source {
                    state.selected_link.set(Some(LinkPick::Authored(link)));
                    state.drag.set(Some(Drag::Rewire { link, from }));
                }
            }
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

/// What a drag does when it is let go, over whatever it was let go on.
///
/// Its own function because there are now four kinds and each commits
/// differently — and because the two that involve a wire are the ones a reader
/// has to be able to compare side by side.
fn finish_drag(state: &Rc<LabState>, drag: Drag, now: &Hit) {
    match drag {
        // A wire commits onto whatever accept pin it was let go over.
        Drag::Wire { from } => {
            if let Hit::Pin { node, dial: false } | Hit::Node(node) = *now {
                if node != from {
                    connect(state, from, node).ok();
                }
            } else {
                state.say("a link needs an accept pin");
            }
        }
        // ★★ R1681 — a picked-up link commits the same way, except that it
        // MOVES rather than being made. Released over nothing it is let go,
        // which is the rule every node editor has and the reference states in
        // as many words: dropping a wire on empty canvas disconnects it.
        Drag::Rewire { link, .. } => match *now {
            Hit::Pin { node, dial: false } | Hit::Node(node) => {
                let landed = state
                    .doc
                    .borrow()
                    .tree(ROOT)
                    .and_then(|t| t.link(link).map(|l| l.to.node));
                if landed == Some(node) {
                    // Picked up and put back down where it was. The reference
                    // has to restore it; here there is nothing to restore,
                    // because a move that has not happened has taken nothing
                    // out.
                    state.say("link unchanged");
                } else {
                    relink_to(state, link, node).ok();
                }
            }
            _ => {
                delete_link(state, link).ok();
            }
        },
        Drag::Node { node, .. } => apply_frame(state, node),
        Drag::Pan { .. } | Drag::Frame { .. } => {}
    }
}

fn release(state: &Rc<LabState>) {
    let (px, py) = state.cursor.get();
    let now = Hit::at(state, px, py);
    let was = state.pressed.borrow_mut().take();
    let drag = state.drag.get();
    state.drag.set(None);

    if let Some(drag) = drag {
        finish_drag(state, drag, &now);
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
        Hit::Reset(scope) => {
            scope.apply(state);
            state.say(format!("{} back to how it opened", scope.wire()));
        }
        // ★★ R1682 — the node's-life seats, through the same three functions
        // the wire calls. A refusal (the last card) has already said so on the
        // toast, which is where a person reads it.
        Hit::NodeAct(act) => {
            if let Some(node) = state.selected.get() {
                let _ = match act {
                    NodeAct::Collapse => collapse_card(state, node),
                    NodeAct::Disable => disable_card(state, node),
                    NodeAct::Delete => delete_card(state, node),
                };
            }
        }
        // ★★ R1683 — one seat, two jobs, which is what makes the field's state
        // readable from the button: shut, it opens on the name; open, it
        // applies what was typed. The reference's box works the same way.
        Hit::Rename | Hit::AddKey => edit_seat(state, &now),
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
        Hit::Link(id) => state.selected_link.set(Some(LinkPick::Authored(id))),
        Hit::Observed(from, to) => state.selected_link.set(Some(LinkPick::Observed(from, to))),
        // ★★ R1681 — one seat, two meanings, chosen by which layer the picked
        // link is in. A drawn one can be removed; a reported one is a fact
        // about the world and the only thing to do with it is put it in the
        // drawing.
        Hit::LinkAct => match state.selected_link.get() {
            Some(LinkPick::Authored(id)) => {
                delete_link(state, id).ok();
            }
            Some(LinkPick::Observed(from, to)) => {
                adopt_link(state, from, to).ok();
            }
            None => {}
        },
        Hit::Endpoint(n) => {
            choose_endpoint(state, n).ok();
        }
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
/// Where a new card can go without covering one that is already there, in the
/// CANVAS coordinates a node is stored in.
///
/// ★ R1656 — the centre, and then straight down until the spot is free.
///
/// It was the centre unconditionally, and a card dropped on top of another is
/// two cards that answer for the same pixels. The first repair searched in
/// WINDOW coordinates and compared against `card_rect`, which is in world ones —
/// `to_canvas` and `to_content` are not inverses, they map different pairs of
/// frames — so nothing ever looked occupied and six added nodes landed in one
/// stack. Measured by the test written for it, which is the point of writing
/// one: the round's own repair was wrong and said so on its first run.
fn free_spot(state: &LabState, want: (i32, i32), size: (i32, i32)) -> (i32, i32) {
    let taken: Vec<(i32, i32, i32, i32)> = {
        let doc = state.doc.borrow();
        doc.tree(ROOT).map_or_else(Vec::new, |tree| {
            state
                .cards()
                .into_iter()
                .filter_map(|node| tree.node(node).map(|held| (held.x, held.y)))
                .map(|(x, y)| (x, y, size.0, size.1))
                .collect()
        })
    };
    let clear = |x: i32, y: i32| {
        taken.iter().all(|(hx, hy, hw, hh)| {
            x >= hx + hw || *hx >= x + size.0 || y >= hy + hh || *hy >= y + size.1
        })
    };
    let step = size.1 + 12;
    let (mut x, mut y) = want;
    // Bounded: a column full to its own depth wraps to the next one rather than
    // looping forever.
    for attempt in 0..64 {
        if clear(x, y) {
            break;
        }
        y += step;
        if attempt % 8 == 7 {
            x += size.0 + 12;
            y = want.1;
        }
    }
    (x, y)
}

fn add_node(state: &Rc<LabState>, role: Role) {
    let canvas = canvas_rect();
    let want = to_canvas(state, canvas.x + canvas.w / 2, canvas.y + canvas.h / 2);
    // Design units, because that is what a node's stored position is in: the
    // card is painted at `zoom` times this, and so is every other card.
    let (cx, cy) = free_spot(state, want, (146, 96));
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
    state.forms.borrow_mut().insert(id, form_for(&name, role));
    // ★ R1679 — where this card came into being, which is the only thing a
    // layout reset can put it back to. A card the specification does not
    // describe has no other source, and without this the layout predicate was
    // blind to it: measured, dragging an added card moved it from [502,476] to
    // [562,512] while `changed.layout` stayed false.
    state.opened_at.borrow_mut().insert(
        id,
        Placement {
            at: (cx, cy),
            host: None,
            // ★ R1682 — `None` is what makes this card a stray to the node
            // reset. Not "the name it happens to have now", which a rename
            // moves; "the name the specification gave it", which nothing does.
            opened_as: None,
        },
    );
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
        // ★ R1656 — the fraction is of the LIVE surface, and this multiplies by
        // the live surface.
        //
        // It multiplied by the design size until a person reported that nodes
        // stop clicking after a maximise. `External::pointer_move` hands a
        // fraction of the widget's post-layout rect and does not hand the rect,
        // so a consumer that wants pixels has to find the basis somewhere else
        // — and `window_size()` reads `use_viewport_size`, which needs a
        // reactive scope. There is none inside a pointer callback, so it fell
        // through to the design constants and every coordinate arrived scaled
        // by opening-size over current-size. Measured exactly: after a maximise
        // to 2494x1531 the app was told 0.5775x horizontally (1440/2494) and
        // 0.5880x vertically (900/1531), so a press aimed at the right-hand
        // inspector landed sixty pixels away and nothing under the cursor
        // answered.
        //
        // `surface` is the size the shell told this widget it has
        // ([`External::on_resize`]), so the basis and the fraction are now one
        // fact rather than two — the class fix is
        // [[debt-an-external-reads-a-fraction-without-its-basis]].
        let (w, h) = self.surface;
        let px = (x_rel.clamp(0.0, 1.0) * w as f32) as u32;
        let py = (y_rel.clamp(0.0, 1.0) * h as f32) as u32;
        move_cursor(&state, px, py);
    }

    /// R1656 §5.15 — the shell's resize notification, which is how this widget
    /// knows what a pointer fraction is a fraction OF.
    fn on_resize(&mut self, width: u32, height: u32) {
        self.surface = (width.max(1), height.max(1));
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
    /// ★★ R1683 — the shared field's posture and caret, which the shell reads
    /// out of the painted scene and hands back to the view. The same contract
    /// the sibling node editor uses, so the field's own external stays the
    /// authority on what it holds and the view never guesses.
    type State = (TextFieldState, u32);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = LabOracle::new();
        oracle.attach(use_lab_state());
        Box::new(oracle)
    }

    /// ★★★ R1683 — the field's own external, mounted beside this screen's.
    ///
    /// `view_field` paints a container; the thing that HOLDS the text, owns
    /// focus, takes a keystroke and answers what it is doing is a separate
    /// external addressed by the same tag. Measured while wiring this: without
    /// it the field painted, the screen's `editing` slot said it was open, and
    /// every keystroke was refused — because the keymap forwards to an external
    /// that was not there. `blur_committing_field_extra` is the lifted
    /// commit-on-blur one the sibling node editor mounts, so a click away from
    /// the box does what a click away from a box does.
    fn create_extra_externals() -> Vec<pinion_core::widget_core::ExtraExternal> {
        vec![pinion_core::widgets::text_field::blur_committing_field_extra(EDIT_TAG)]
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32) {
        tf_paint::read_text_field_state(scene, EDIT_TAG)
    }

    fn view(state: (TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, *frame)
    }

    /// ★★★ R1683 — the keystroke path, and it is the framework's keymap rather
    /// than another copy of one.
    ///
    /// `edit_field_keymap` is the lifted SSOT the data grid, the property grid
    /// and the sibling node editor already share; this is its FOURTH call site.
    /// Enter commits through the same verb the wire calls, Escape closes and
    /// leaves the value alone, and a named key this screen wants while the
    /// field is shut (Space to run, +/- to zoom) is deliberately NOT reached
    /// while it is open — a person typing a name must be able to type a space.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        if focused != Some(EDIT_TAG) {
            return false;
        }
        let state = use_lab_state();
        let kind = state.editing.get().map_or(CellKind::Text, Editing::kind);
        edit_field_keymap(
            scene,
            EDIT_TAG,
            key,
            modifiers,
            kind,
            || {
                commit_edit(&state).ok();
            },
            || end_edit(&state),
        )
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
    fn access_node(_state: &(TextFieldState, u32), _focused: Option<&str>) -> Vec<AccessNode> {
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
