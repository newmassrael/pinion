// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-topology-view` — R1947 §5.27 §5.40 §5.41 — the analysis tool's
//! **topology section**, the sixth seat of the reference's rail.
//!
//! ## What forced this example, and why it is not like the four before it
//!
//! Every earlier section closed a gap against the SCOPE reference — the
//! first-stage screen mockup, whose rail this build reproduces seat for seat.
//! This one does not. That mockup draws this seat LOCKED, booked under a later
//! requirement, and this build locked it the same way for 219 rounds; the two
//! agree, and `docs/analyzer-rail-spec.json`'s `owed` was empty.
//!
//! What the mockup cannot see is that the BEHAVIOUR reference — the working
//! prototype, a superset of it — **builds** this section. R1946 measured that
//! (eight section handlers, eight selected-predicates, a rail with no locked
//! seat, a full topology view and a sessions view) and turned the difference
//! into a list. This crate pays off the first of its two entries.
//!
//! So opening this seat is the second stage arriving rather than a
//! reproduction gap closing, which is exactly the order the owner set on
//! 2026-08-19: reproduce the first stage, then improve past it. The rail pin
//! records the divergence instead of hiding it.
//!
//! ## The screen
//!
//! ```text
//! cargo run -p hello-topology-view --release
//! ```
//!
//! Three panes. A 238-wide filter rail: the layout choice, two link toggles, a
//! highlighted key pattern and the live-capture toggle. A graph column whose
//! 46-high header names the section, states which layout is in force, marks the
//! capture live and offers `Fit`; under it the plot, six peers around one
//! router, with zoom controls and a selection hint. A 308-wide inspector: the
//! picked node's identity, a four-tile measurement grid, the key patterns
//! observed on it, and two actions drawn and refused.
//!
//! Click a node to inspect it; click a layout to rearrange; click a toggle to
//! drop a class of link; the arrows walk the plot.
//!
//! ## Two things here are this section's own
//!
//! * ★ **A node's place is per mille of the plot, not a pixel.** The reference
//!   lays out in a fixed 900x560 space and fits it to whatever the column is,
//!   so its numbers mean nothing at another aspect ratio. Normalised once in
//!   `spec`, a placement survives a resize, a zoom and a different window
//!   with no one re-deriving it, and the arithmetic stays in integers.
//! * ★ **The router is a node, not a special case.** The reference draws it
//!   beside the loop that draws every other node, in markup that differs only
//!   in the shape. Here `spec::NodeSpec::is_router` decides the shape, so the
//!   population is one list, the inspector needs no second path, and a capture
//!   that observed two routers would draw both.

mod judge;
mod spec;

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use pinion_a11y::{AccessFocus, AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::describe::Descriptions;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, PointerTarget, ReadRefusal, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::style::{
    Border, BoxStyle, Color, LayoutStyle, PathStyle, Size, Stroke, TextOverflow, TextStyle,
};
use pinion_core::theme::use_theme;
use pinion_core::voice::Silence;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::run::text_run;

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTopologyViewRenderer, HelloTopologyViewRendererError);

// ── Tags ────────────────────────────────────────────────────────────────────

/// The screen's paint-root tag, and the tag its external is keyed by.
const VIEW_TAG: &str = "topology_view";

/// The root group every mark of this screen hangs under.
const ROOT_TAG: &str = "tv.root";

/// The theme this screen reads its palette from.
const THEME_TAG: &str = "app";

/// The filter rail's own group.
const FILTER_TAG: &str = "tv.filters";
/// The graph column's own group.
const GRAPH_TAG: &str = "tv.graph";
/// The inspector's own group.
const INSPECTOR_TAG: &str = "tv.inspector";
/// The plot, which is the graph column's pressable body.
const CANVAS_TAG: &str = "tv.graph.canvas";
/// Where a resting description is painted and announced.
const TOOLTIP_TAG: &str = "tv.tip";

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;
const FILTER_W: u32 = spec::FILTER_W;
const INSPECTOR_W: u32 = spec::INSPECTOR_W;
const HEADER_H: u32 = spec::HEADER_H;
const PAD: u32 = spec::PAD;
const GAP: u32 = spec::GAP;
const ROW_H: u32 = spec::ROW_H;
const GROUP_HEAD_H: u32 = spec::GROUP_HEAD_H;

const FONT_TINY: u32 = spec::FONT_TINY;
const FONT_SMALL: u32 = spec::FONT_SMALL;
const FONT_BODY: u32 = spec::FONT_BODY;
const FONT_TITLE: u32 = spec::FONT_TITLE;
const FONT_HEADLINE: u32 = spec::FONT_HEADLINE;

/// The narrowest this section can be drawn without the plot losing its
/// meaning: both side panes at their declared widths plus a plot wide enough to
/// hold the graph's own spread.
///
/// ⚠ The first draft allowed 180 for the plot, and the sweep at that floor drew
/// a graph column narrower than the controls painted over it — the zoom buttons
/// and the selection hint changed places, because at that width they no longer
/// sit at opposite ends of anything. A floor that produces a scrambled screen
/// is not a floor.
const MIN_W: u32 = FILTER_W + INSPECTOR_W + 360;
/// The shortest, which is what the inspector's own content needs.
const MIN_H: u32 = 480;

const _: () = assert!(
    MIN_W < WIN_W && MIN_H < WIN_H,
    "a floor at or above the opening size is not a floor"
);

/// ★ What this screen needs and what it gives up when it does not get it.
///
/// Panning rather than conceding: the plot is a *place*, and a plot that drops
/// a node when the window narrows is a plot that lies about the capture. So the
/// section keeps its whole content and lets a host pan it, which is the policy
/// the sibling sections reached for the same reason.
const SHRINK: ShrinkPolicy = ShrinkPolicy::panning((MIN_W, MIN_H), (720, 380));

/// The extent this section is laying out in.
///
/// ⚠ Read from the framework rather than assumed to be [`WIN_W`] x [`WIN_H`].
/// The first draft returned the declared pair, and the sweep caught it at the
/// first size that was not that pair: the inspector, positioned from the
/// window's right edge, ran off a narrower window entirely. A geometry that
/// asks nobody how big the window is looks correct at exactly one size.
fn window_size() -> (u32, u32) {
    pinion_core::external::layout_size(VIEW_TAG, SHRINK.comfortable(), (WIN_W, WIN_H))
}

/// The filter rail.
fn filter_rect() -> Rect {
    let (_, h) = window_size();
    Rect::new(0, 0, FILTER_W, h)
}

/// The inspector.
fn inspector_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(w.saturating_sub(INSPECTOR_W), 0, INSPECTOR_W, h)
}

/// The graph column — everything between the two side panes.
fn graph_rect() -> Rect {
    let (w, h) = window_size();
    let wide = w
        .saturating_sub(FILTER_W)
        .saturating_sub(INSPECTOR_W)
        .max(1);
    Rect::new(FILTER_W, 0, wide, h)
}

/// The graph column's header.
fn graph_header_rect() -> Rect {
    let col = graph_rect();
    Rect::new(col.x, col.y, col.w, HEADER_H)
}

/// The plot itself.
fn canvas_rect() -> Rect {
    let col = graph_rect();
    Rect::new(
        col.x,
        col.y + HEADER_H,
        col.w,
        col.h.saturating_sub(HEADER_H),
    )
}

/// ★★★★★ R1947 — **where a per-mille place lands on this frame, at this zoom.**
///
/// One derivation, used by the painter and by the hit test both, so what a
/// pointer reaches and what a reader sees cannot drift. The zoom is applied
/// about the plot's centre, which is what makes zooming in on a graph feel like
/// moving closer to it rather than sliding it off the pane.
fn plot_point(at: (u32, u32), zoom: u32) -> (i64, i64) {
    let plot = canvas_rect();
    let span = i64::from(spec::PLOT_SPAN);
    let zoom = i64::from(zoom);
    let centre_x = i64::from(plot.x) + i64::from(plot.w) / 2;
    let centre_y = i64::from(plot.y) + i64::from(plot.h) / 2;
    let x = i64::from(plot.x) + i64::from(plot.w) * i64::from(at.0) / span;
    let y = i64::from(plot.y) + i64::from(plot.h) * i64::from(at.1) / span;
    (
        centre_x + (x - centre_x) * zoom / 100,
        centre_y + (y - centre_y) * zoom / 100,
    )
}

/// ★★★★★ R1947 — **the closest this plot may be drawn and still hold every
/// node it declares, DERIVED from the plot it is being drawn in.**
///
/// [`spec::ZOOM_MAX`] is a declared ceiling; this is the one physics allows,
/// and the zoom takes the smaller. The sweep is what forced it: at the declared
/// ceiling the outermost peer was painted 95 pixels past the plot's right edge
/// — drawn, reachable, announced, and not on the screen. A person zooming in
/// would have watched a third of the capture leave the pane.
///
/// The reference gets away with a fixed ceiling because its plot is an SVG that
/// CLIPS, so its overflow is invisible rather than absent; ours would paint
/// over the inspector. Deriving the bound is the better answer either way: it
/// moves with the window, so a narrow window zooms less rather than lying.
fn zoom_ceiling() -> u32 {
    let plot = canvas_rect();
    let span = i64::from(spec::PLOT_SPAN);
    let base = i64::from(node_side(100)) / 2;
    let mut ceiling = i64::from(spec::ZOOM_MAX);
    for node in spec::NODES {
        for layout in spec::LAYOUTS {
            let (mx, my) = node.at(layout);
            let half = if node.is_router() { base * 2 } else { base };
            for (offset, radius, extent) in [
                (i64::from(mx), half, i64::from(plot.w)),
                (i64::from(my), base, i64::from(plot.h)),
            ] {
                // How far this node's edge is from the plot's centre, at 100%.
                let from_centre = (extent * offset / span - extent / 2).abs() + radius;
                if from_centre > 0 {
                    ceiling = ceiling.min(100 * (extent / 2) / from_centre);
                }
            }
        }
    }
    u32::try_from(ceiling.max(i64::from(spec::ZOOM_MIN))).unwrap_or(spec::ZOOM_FIT)
}

/// How far across a node is drawn, at this zoom.
fn node_side(zoom: u32) -> u32 {
    let plot = canvas_rect();
    let shorter = plot.w.min(plot.h);
    let side = u64::from(shorter) * u64::from(spec::NODE_R) * 2 / u64::from(spec::PLOT_SPAN);
    let scaled = side * u64::from(zoom) / 100;
    u32::try_from(scaled.clamp(8, u64::from(shorter))).unwrap_or(8)
}

/// The box a node is drawn in.
///
/// A router is drawn wider than it is tall — the reference's rounded plate —
/// and every other node is a circle, which is a square with a radius of half
/// its side.
fn node_rect(node: &spec::NodeSpec, layout: spec::Layout, zoom: u32) -> Rect {
    let (cx, cy) = plot_point(node.at(layout), zoom);
    let side = node_side(zoom);
    let (w, h) = if node.is_router() {
        (side * 2, side)
    } else {
        (side, side)
    };
    let x = (cx - i64::from(w) / 2).max(0);
    let y = (cy - i64::from(h) / 2).max(0);
    Rect::new(
        u32::try_from(x).unwrap_or(0),
        u32::try_from(y).unwrap_or(0),
        w,
        h,
    )
}

/// The ring drawn around the picked node.
fn node_ring_rect(node: &spec::NodeSpec, layout: spec::Layout, zoom: u32) -> Rect {
    let body = node_rect(node, layout, zoom);
    let grow = node_side(zoom) * (spec::RING_R - spec::NODE_R) / spec::NODE_R;
    Rect::new(
        body.x.saturating_sub(grow / 2),
        body.y.saturating_sub(grow / 2),
        body.w + grow,
        body.h + grow,
    )
}

/// Where the zoom controls sit — bottom left of the plot, as the reference has
/// them.
fn zoom_rect(nth: u32) -> Rect {
    let plot = canvas_rect();
    let side = 32;
    Rect::new(
        plot.x + PAD,
        plot.y + plot.h.saturating_sub(PAD + side * 2 + 6) + nth * (side + 6),
        side,
        side,
    )
}

/// Where the selection hint sits — bottom right, as the reference has it.
fn hint_rect() -> Rect {
    let plot = canvas_rect();
    let wide = 190;
    Rect::new(
        plot.x + plot.w.saturating_sub(PAD + wide),
        plot.y + plot.h.saturating_sub(PAD + 14),
        wide,
        14,
    )
}

/// The `Fit` control in the graph header.
fn fit_rect() -> Rect {
    let head = graph_header_rect();
    Rect::new(head.x + head.w.saturating_sub(PAD + 56), head.y + 8, 56, 30)
}

/// The live mark in the graph header, left of `Fit`.
fn live_rect() -> Rect {
    let head = graph_header_rect();
    Rect::new(
        head.x + head.w.saturating_sub(PAD + 56 + 8 + 54),
        head.y + 15,
        54,
        16,
    )
}

/// A group heading in the filter rail, by ordinal from the top.
fn group_head_rect(nth: u32) -> Rect {
    let rail = filter_rect();
    Rect::new(
        rail.x + PAD,
        rail.y + group_top(nth),
        rail.w.saturating_sub(PAD * 2),
        GROUP_HEAD_H,
    )
}

/// Where the `nth` group of the filter rail starts.
///
/// Derived from the groups above it rather than tabled, so a group that gains a
/// row moves the ones under it instead of overlapping them — the defect a
/// hand-kept table of tops produces the first time anything changes.
fn group_top(nth: u32) -> u32 {
    let mut top = 78;
    for before in 0..nth {
        top += GROUP_HEAD_H + group_body_h(before) + 26;
    }
    top
}

/// How tall the `nth` group's body is.
fn group_body_h(nth: u32) -> u32 {
    match nth {
        // Show links: one row per link toggle.
        1 => ROW_H * 2 + GAP,
        // Highlight: the pattern box and a row of chips.
        2 => ROW_H + GAP + 24,
        // Layout (0) and Streaming (3): one control each.
        _ => ROW_H,
    }
}

/// One row of a group's body.
fn group_row_rect(group: u32, nth: u32) -> Rect {
    let rail = filter_rect();
    Rect::new(
        rail.x + PAD,
        rail.y + group_top(group) + GROUP_HEAD_H + nth * (ROW_H + GAP),
        rail.w.saturating_sub(PAD * 2),
        ROW_H,
    )
}

/// One button of the layout segmented control.
fn layout_rect(nth: u32) -> Rect {
    let row = group_row_rect(0, 0);
    let each = row.w / 2;
    Rect::new(row.x + nth * each, row.y, each, row.h)
}

/// The switch at the right of a toggle row.
fn switch_rect(row: Rect) -> Rect {
    Rect::new(row.x + row.w.saturating_sub(38), row.y + 6, 38, 20)
}

/// Which row of the rail a toggle is drawn in.
fn toggle_row(nth: usize) -> Rect {
    let toggle = &spec::TOGGLES[nth];
    if toggle.group == "links" {
        group_row_rect(1, u32::try_from(nth).unwrap_or(0))
    } else {
        group_row_rect(3, 0)
    }
}

/// A chip beside the highlighted key pattern.
fn chip_rect(nth: u32) -> Rect {
    let row = group_row_rect(2, 0);
    let each = 86;
    Rect::new(row.x + nth * (each + 6), row.y + ROW_H + GAP, each, 24)
}

/// One of the inspector's four measurement tiles.
fn tile_rect(nth: u32) -> Rect {
    let pane = inspector_rect();
    let wide = (pane.w.saturating_sub(PAD * 2 + 10)) / 2;
    let tall = 54;
    Rect::new(
        pane.x + PAD + (nth % 2) * (wide + 10),
        192 + (nth / 2) * (tall + 10),
        wide,
        tall,
    )
}

/// One of the inspector's two actions.
fn action_rect(nth: u32) -> Rect {
    let pane = inspector_rect();
    let wide = (pane.w.saturating_sub(PAD * 2 + 8)) / 2;
    let (_, h) = window_size();
    Rect::new(
        pane.x + PAD + nth * (wide + 8),
        h.saturating_sub(PAD + 34),
        wide,
        34,
    )
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

// ── Ink ─────────────────────────────────────────────────────────────────────

const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// The colours this section paints with.
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// The window's ground.
    ground: Color,
    /// A pane's ground.
    surface: Color,
    /// A control's ground.
    raised: Color,
    /// A pane's edge.
    border: Color,
    /// A control's edge.
    edge: Color,
    /// Body text.
    text: Color,
    /// Secondary text.
    dim: Color,
    /// The faintest text — a caption.
    faint: Color,
    /// The accent a chosen control wears.
    accent: Color,
    /// Text on the accent.
    accent_fg: Color,
    /// A healthy standing.
    ok: Color,
    /// A standing that needs attention.
    warn: Color,
    /// A standing that has failed.
    err: Color,
}

fn ink() -> Ink {
    let dark = use_theme(THEME_TAG).is_dark();
    if dark {
        Ink {
            ground: rgb(0x14_16_1A),
            surface: rgb(0x1A_1D_22),
            raised: rgb(0x22_26_2C),
            border: rgb(0x2C_31_38),
            edge: rgb(0x3A_40_49),
            text: rgb(0xE6_E9_EE),
            dim: rgb(0xA8_AF_BA),
            faint: rgb(0x76_7E_8A),
            accent: rgb(0x2D_6C_DF),
            accent_fg: rgb(0x8A_B4_FF),
            ok: rgb(0x35_C0_8B),
            warn: rgb(0xC7_78_00),
            err: rgb(0xE0_4F_5F),
        }
    } else {
        Ink {
            ground: rgb(0xF6_F7_F9),
            surface: rgb(0xFF_FF_FF),
            raised: rgb(0xF0_F2_F5),
            border: rgb(0xDD_E1_E6),
            edge: rgb(0xC4_CB_D4),
            text: rgb(0x1B_1F_24),
            dim: rgb(0x4C_54_5F),
            faint: rgb(0x78_81_8D),
            accent: rgb(0x2D_6C_DF),
            accent_fg: rgb(0x1B_4F_B5),
            ok: rgb(0x1F_8A_4C),
            warn: rgb(0xA9_66_00),
            err: rgb(0xC2_35_45),
        }
    }
}

/// The colour a standing is drawn in.
fn standing_ink(standing: spec::Standing, ink: Ink) -> Color {
    match standing {
        spec::Standing::Active => ink.ok,
        spec::Standing::Serving => ink.accent,
        spec::Standing::Reconnecting => ink.warn,
        spec::Standing::Down => ink.err,
    }
}

/// The colour a link is drawn in.
fn link_ink(kind: spec::LinkKind, ink: Ink) -> Color {
    match kind {
        spec::LinkKind::Data | spec::LinkKind::Slow => ink.accent,
        spec::LinkKind::Mesh => ink.edge,
        spec::LinkKind::Strained => ink.warn,
        spec::LinkKind::Down => ink.err,
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// What this section holds between frames.
#[derive(Debug)]
struct ViewState {
    /// Which of [`spec::LAYOUTS`] is in force.
    layout: Signal<usize>,
    /// The node the inspector is showing.
    selected: Signal<String>,
    /// The plot's zoom, as a percent.
    zoom: Signal<u32>,
    /// One flag per entry of [`spec::TOGGLES`], in that order.
    toggles: Signal<Vec<bool>>,
    /// Whether a pointer is over this screen at all — which is a different fact
    /// from where it last was, and is what takes a resting description off the
    /// frame.
    pointer_inside: Signal<bool>,
    /// Where the pointer is resting, when it is inside.
    resting: Signal<Option<String>>,
    /// What the section last said about itself.
    said: Signal<String>,
}

impl ViewState {
    fn new() -> Self {
        Self {
            layout: Signal::new(0),
            selected: Signal::new(spec::OPENS_ON.to_owned()),
            zoom: Signal::new(spec::ZOOM_FIT),
            toggles: Signal::new(spec::TOGGLES.iter().map(|t| t.opens_on).collect()),
            pointer_inside: Signal::new(false),
            resting: Signal::new(None),
            said: Signal::new(String::new()),
        }
    }

    /// The layout in force.
    fn layout(&self) -> spec::Layout {
        spec::LAYOUTS[self.layout.get().min(spec::LAYOUTS.len() - 1)]
    }

    /// The node the inspector is showing. Never `None`: the section opens on a
    /// node and every way of changing the selection picks another one, so an
    /// empty inspector is a state this screen cannot reach.
    fn picked(&self) -> &'static spec::NodeSpec {
        spec::node(&self.selected.get()).unwrap_or(&spec::NODES[0])
    }

    /// Whether the `nth` toggle is on.
    fn toggle(&self, nth: usize) -> bool {
        self.toggles.get().get(nth).copied().unwrap_or(false)
    }

    /// Whether a link of this kind is drawn right now.
    fn draws(&self, kind: spec::LinkKind) -> bool {
        if kind.is_mesh() {
            return self.toggle(0);
        }
        if kind.is_down() {
            return self.toggle(1);
        }
        true
    }

    /// Whether the capture is running, which is the third toggle.
    fn capturing(&self) -> bool {
        self.toggle(2)
    }

    /// What the section last said, for the wire and for a reader.
    fn said_sentence(&self) -> String {
        self.said.get()
    }

    fn say(&self, sentence: impl Into<String>) {
        self.said.set(sentence.into());
    }
}

thread_local! {
    static VIEW: RefCell<Option<Rc<ViewState>>> = const { RefCell::new(None) };
}

/// This screen's state, created on first use.
fn use_view_state() -> Rc<ViewState> {
    VIEW.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(Rc::new(ViewState::new()));
        }
        slot.as_ref().expect("the state was just created").clone()
    })
}

// ── The hit test ────────────────────────────────────────────────────────────

/// What is under a point. One enum, resolved from the same rectangles the
/// painter uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// A node of the plot, by its index in [`spec::NODES`].
    Node(usize),
    /// A button of the layout segmented control.
    Layout(usize),
    /// A switch of the filter rail.
    Toggle(usize),
    /// A chip beside the highlighted key pattern.
    Chip(usize),
    /// The `Fit` control.
    Fit,
    /// A zoom control: `true` is in.
    Zoom(bool),
    /// An inspector action, which refuses.
    Action(usize),
    /// Nothing pressable.
    Nothing,
}

impl Hit {
    /// What is at a point of this screen's own space.
    fn at(state: &Rc<ViewState>, px: u32, py: u32) -> Self {
        for (n, _) in spec::LAYOUTS.iter().enumerate() {
            if contains(layout_rect(u32::try_from(n).unwrap_or(0)), px, py) {
                return Self::Layout(n);
            }
        }
        for (n, _) in spec::TOGGLES.iter().enumerate() {
            if contains(switch_rect(toggle_row(n)), px, py) {
                return Self::Toggle(n);
            }
        }
        for (n, _) in spec::HIGHLIGHT_CHIPS.iter().enumerate() {
            if contains(chip_rect(u32::try_from(n).unwrap_or(0)), px, py) {
                return Self::Chip(n);
            }
        }
        if contains(fit_rect(), px, py) {
            return Self::Fit;
        }
        if contains(zoom_rect(0), px, py) {
            return Self::Zoom(true);
        }
        if contains(zoom_rect(1), px, py) {
            return Self::Zoom(false);
        }
        for (n, _) in spec::ACTIONS.iter().enumerate() {
            if contains(action_rect(u32::try_from(n).unwrap_or(0)), px, py) {
                return Self::Action(n);
            }
        }
        // ★ The plot last, so a control drawn over it wins — and in reverse
        // order of the population, so a node drawn later (and therefore on top)
        // is the one a press reaches. Two nodes that overlap must resolve the
        // way they are painted or the pointer disagrees with the eye.
        let (layout, zoom) = (state.layout(), state.zoom.get());
        for (n, node) in spec::NODES.iter().enumerate().rev() {
            if contains(node_rect(node, layout, zoom), px, py) {
                return Self::Node(n);
            }
        }
        Self::Nothing
    }

    /// What is under a paint tag — the same answer by another address, which is
    /// what lets a press by tag and a press by point be one behaviour.
    fn of_tag(tag: &str) -> Self {
        if let Some(id) = tag.strip_prefix("tv.node.")
            && let Some(n) = spec::NODES.iter().position(|node| node.id == id)
        {
            return Self::Node(n);
        }
        if let Some(rest) = tag.strip_prefix("tv.layout.")
            && let Ok(n) = rest.parse::<usize>()
            && n < spec::LAYOUTS.len()
        {
            return Self::Layout(n);
        }
        if let Some(key) = tag.strip_prefix("tv.toggle.")
            && let Some(n) = spec::TOGGLES.iter().position(|t| t.key == key)
        {
            return Self::Toggle(n);
        }
        if let Some(rest) = tag.strip_prefix("tv.chip.")
            && let Ok(n) = rest.parse::<usize>()
            && n < spec::HIGHLIGHT_CHIPS.len()
        {
            return Self::Chip(n);
        }
        if let Some(key) = tag.strip_prefix("tv.inspector.")
            && let Some(n) = spec::ACTIONS.iter().position(|a| a.key == key)
        {
            return Self::Action(n);
        }
        match tag {
            "tv.graph.fit" => Self::Fit,
            "tv.graph.zoom_in" => Self::Zoom(true),
            "tv.graph.zoom_out" => Self::Zoom(false),
            _ => Self::Nothing,
        }
    }

    /// The word this hit answers on the wire.
    fn word(&self) -> Option<String> {
        match self {
            Self::Node(n) => Some(format!("node:{}", spec::NODES[*n].id)),
            Self::Layout(n) => Some(format!("layout:{}", spec::LAYOUTS[*n].in_force())),
            Self::Toggle(n) => Some(format!("toggle:{}", spec::TOGGLES[*n].key)),
            Self::Chip(n) => Some(format!("chip:{}", spec::HIGHLIGHT_CHIPS[*n])),
            Self::Fit => Some("fit".to_owned()),
            Self::Zoom(true) => Some("zoom_in".to_owned()),
            Self::Zoom(false) => Some("zoom_out".to_owned()),
            Self::Action(n) => Some(format!("action:{}", spec::ACTIONS[*n].key)),
            Self::Nothing => None,
        }
    }
}

// ── The handlers a press and the wire both reach ────────────────────────────

/// Pick a node.
fn select_node(state: &Rc<ViewState>, nth: usize) {
    let node = &spec::NODES[nth];
    state.selected.set(node.id.to_owned());
    state.say(format!("{} selected", node.id));
}

/// Choose a layout.
fn choose_layout(state: &Rc<ViewState>, nth: usize) {
    state.layout.set(nth.min(spec::LAYOUTS.len() - 1));
    state.say(format!("{} layout", spec::LAYOUTS[nth].in_force()));
}

/// Flip a switch.
fn flip_toggle(state: &Rc<ViewState>, nth: usize) {
    let mut flags = state.toggles.get();
    if let Some(flag) = flags.get_mut(nth) {
        *flag = !*flag;
        let on = *flag;
        state.toggles.set(flags);
        state.say(format!(
            "{} {}",
            spec::TOGGLES[nth].title,
            if on { "shown" } else { "hidden" }
        ));
    }
}

/// Zoom the plot, or return it to the fit.
fn zoom_by(state: &Rc<ViewState>, closer: bool) {
    let now = state.zoom.get();
    let next = if closer {
        (now + spec::ZOOM_STEP).min(zoom_ceiling())
    } else {
        now.saturating_sub(spec::ZOOM_STEP).max(spec::ZOOM_MIN)
    };
    state.zoom.set(next);
    state.say(format!("zoom {next} percent"));
}

/// Return the plot to the zoom it opened at.
fn fit(state: &Rc<ViewState>) {
    state.zoom.set(spec::ZOOM_FIT);
    state.say("fit".to_owned());
}

/// ★ An action that refuses, in the words of the requirement that books it.
///
/// Not silence and not a crash: the reference draws these live and answers a
/// later-stage toast, so a build that drew nothing would lose the affordance
/// and one that acted would promise a section nobody has.
fn refuse_action(state: &Rc<ViewState>, nth: usize) {
    let action = &spec::ACTIONS[nth];
    state.say(format!(
        "{} is not in this release - booked under {}",
        action.title, action.reserved_for
    ));
}

/// Apply a hit, wherever it came from.
fn press(state: &Rc<ViewState>, hit: &Hit) -> bool {
    match hit {
        Hit::Node(n) => {
            select_node(state, *n);
            true
        }
        Hit::Layout(n) => {
            choose_layout(state, *n);
            true
        }
        Hit::Toggle(n) => {
            flip_toggle(state, *n);
            true
        }
        Hit::Chip(n) => {
            state.say(format!("{} highlighted", spec::HIGHLIGHT_CHIPS[*n]));
            true
        }
        Hit::Fit => {
            fit(state);
            true
        }
        Hit::Zoom(closer) => {
            zoom_by(state, *closer);
            true
        }
        Hit::Action(n) => {
            refuse_action(state, *n);
            true
        }
        Hit::Nothing => false,
    }
}

/// Walk the plot from the keyboard.
fn key_at(state: &Rc<ViewState>, chord: &str) -> bool {
    let here = spec::NODES
        .iter()
        .position(|n| n.id == state.selected.get())
        .unwrap_or(0);
    let last = spec::NODES.len() - 1;
    let next = match chord {
        "ArrowRight" | "ArrowDown" => {
            if here == last {
                0
            } else {
                here + 1
            }
        }
        "ArrowLeft" | "ArrowUp" => {
            if here == 0 {
                last
            } else {
                here - 1
            }
        }
        "Home" => 0,
        "End" => last,
        _ => return false,
    };
    select_node(state, next);
    true
}

// ── Scene helpers ───────────────────────────────────────────────────────────

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

fn run_style(px: u32, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

/// ★★★★★ R1947 — **a run's box is as tall as its own face, always.**
///
/// The caller gives a place and a width; the HEIGHT comes from
/// [`pinion_core::containment::line_rect`], which derives it from the type
/// size. Every height passed in is discarded, and that is the point: this
/// section's first sweep reported **53 of 57 runs** in a box too short for the
/// letters in it — not 53 authoring slips but one convention, applied
/// everywhere, that never consulted the face. A helper that cannot be called
/// wrongly is the only version of that rule anybody keeps.
fn label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    let seat = pinion_core::containment::line_rect(rect.x, rect.y, rect.w, px);
    Scene::Text(TextNode::styled(text.into(), seat, run_style(px, fg)).with_layout(absolute(seat)))
}

fn tagged_label(
    tag: &str,
    text: impl Into<String>,
    rect: Rect,
    px: u32,
    fg: Color,
    silence: Silence,
) -> Scene {
    let seat = pinion_core::containment::line_rect(rect.x, rect.y, rect.w, px);
    text_run(tag, text, seat, run_style(px, fg)).silenced(silence)
}

const FRAME: u32 = 1;

fn panel(tag: &str, rect: Rect, fill: Color, border: Option<Color>, children: Vec<Scene>) -> Scene {
    let mut style = BoxStyle::filled(fill);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, FRAME));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

/// A drawn box that carries no words of its own.
///
/// ★★★★★ R1947 — **the silence is an argument, not a default.** Every box on
/// this screen is either part of a control a reader is told about or decoration
/// that repeats something already said, and which one it is cannot be guessed
/// from the shape. The shell's voice census judges 2,600 regions and reported
/// 90 of this section's unclassified on its first assembly — each one a reader
/// who is not told something the screen paints. A parameter is what makes the
/// author answer.
fn box_at(
    tag: &str,
    rect: Rect,
    fill: Color,
    border: Option<Color>,
    radius: u32,
    silence: Option<Silence>,
) -> Scene {
    let mut style = BoxStyle::filled(fill).with_corner_radius(radius);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, FRAME));
    }
    let node = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    );
    // ★ `None` is not "no answer" — it is the statement that this box speaks
    // for itself through an `AccessNode` with a name, which is a different and
    // stronger thing than being silent. The census tells the two apart, so an
    // author who chose `None` for a box with no access node is reported rather
    // than believed.
    match silence {
        Some(why) => node.silenced(why),
        None => node,
    }
}

/// A seat for one line of `px` type — the box and the run in it are the same
/// height, derived, so a part cannot be shorter than the words it holds.
fn seat(x: u32, y: u32, w: u32, px: u32) -> Rect {
    pinion_core::containment::line_rect(x, y, w, px)
}

/// One part of a specified surface: the tag the specification compares, on a
/// box that HOLDS that part's marks, in the part's own coordinates.
///
/// ★★★★★ R1947 — **a box and the words in it, bound rather than adjacent.**
///
/// `pinion_widget_paint::caption::captioned` builds both and ties the run to
/// the box in the scene, so what a reader sees as one thing is one thing a gate
/// can read. The alternative — a box and a label pushed as siblings — pairs
/// them by *where they landed*, and the shell's own ratchet counts those: this
/// section's first assembly took the application from 148 such pairs to 173.
///
/// Every single-run control on this screen goes through here.
fn captioned_box(
    tag: &str,
    rect: Rect,
    fill: Color,
    border: Option<Color>,
    radius: u32,
    words: (&str, u32, Color),
    silence: Option<Silence>,
) -> Scene {
    let mut style = BoxStyle::filled(fill).with_corner_radius(radius);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, FRAME));
    }
    let (text, px, fg) = words;
    // ★★★★★ The RUN's voice is the BOX's, not a pointer at the box.
    //
    // Two drafts got this wrong in the same way and the census named both.
    // `name_of(tag)` says *that tag carries my name*, and half these boxes have
    // no name of their own; `part_of(tag)` says *I am part of that box*, and a
    // box that is itself only a part of something else leaves the run pointing
    // at a silent middle. Fourteen regions came back **dangling** each time.
    //
    // What is true is simpler: a caption and its box are ONE thing to a reader,
    // so they answer to the same thing. Where the box speaks for itself (an
    // access node names it) the caption is part of it; where the box is part of
    // a named surface, so is the caption.
    let caption_voice = silence
        .clone()
        .unwrap_or_else(|| Silence::part_of(tag.to_owned()));
    let caption = pinion_widget_paint::caption::Caption::new(text, run_style(px, fg))
        .centred()
        .silent(caption_voice);
    let node = pinion_widget_paint::caption::captioned(
        tag,
        rect,
        style,
        &caption,
        pinion_widget_paint::caption::Pointer::Transparent,
    )
    .0;
    match silence {
        Some(why) => node.silenced(why),
        None => node,
    }
}

/// ★★★★★ R1947 — **a part is NAMED, not silenced.**
///
/// The first draft gave every part box a `layout` silence — *it says nothing
/// itself and what it contains does* — and the shell's voice census answered
/// **54 hollow**: the promise was false, because what a part contains is
/// untagged runs, which say nothing the census can hear. A `layout` silence
/// over a mute subtree is exactly the escape hatch that census exists to
/// refuse, and it refused it.
///
/// The honest repair is the one the specification already made possible: each
/// part has a `title` in the pin, so [`access_nodes`] derives an `AccessNode`
/// for every part of all three surfaces from the same table `judge` titles
/// from. A part added to the specification arrives named.
fn part_box(tag: &str, rect: Rect, children: Vec<Scene>) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_layout(absolute(rect)),
    )
}

/// A straight run between two points of the window, drawn as a stroked path.
fn line(rect: Rect, from: (i64, i64), to: (i64, i64), ink: Color, width: u32) -> Scene {
    // ★ Narrowed to `i16` before the conversion rather than cast from `i64`:
    // every value here is a window coordinate, so it fits, and `f32::from(i16)`
    // is exact where `i64 as f32` silently is not. A path point that lost
    // precision would draw a line beside the sockets it claims to join.
    let local = |at: (i64, i64)| {
        let narrow = |v: i64| f32::from(i16::try_from(v).unwrap_or(0));
        PathPoint::new(
            narrow(at.0 - i64::from(rect.x)),
            narrow(at.1 - i64::from(rect.y)),
        )
    };
    Scene::Path(
        PathNode::new(
            rect,
            vec![
                PathCommand::MoveTo(local(from)),
                PathCommand::LineTo(local(to)),
            ],
            PathStyle::stroked(Stroke::new(ink, width)),
        )
        .with_layout(absolute(rect)),
    )
}

// ── The filter rail ─────────────────────────────────────────────────────────

/// ★★★★★ R1947 — **what the capture observed, counted by GRADE rather than
/// stated as a total.**
///
/// The reference's heading says `6 nodes / 8 links observed` and stops there,
/// so a reader looking at the rail cannot tell a healthy capture from one where
/// a third of it is failing without going and counting circles. Both counts
/// here are derived, and the graded half is derived through the same scale the
/// rest of this application grades by — so a standing whose severity the scale
/// does not hold cannot be counted as healthy by accident, which is what
/// `tests::r1947_every_standing_is_graded_by_the_scale_this_application_uses`
/// holds it to.
///
/// ⚠ Second-stage work, and kept deliberately: the reference does not do this,
/// and the owner's ordering rule says what this build has and the reference
/// does not is not removed. The reproduction is intact — the first clause is
/// the reference's own sentence, unchanged.
fn observed_sentence() -> String {
    let degraded = spec::NODES
        .iter()
        .filter(|n| spec::SEVERITY.rank(n.standing.severity()).unwrap_or(0) > 0)
        .count();
    let strained = spec::LINKS
        .iter()
        .filter(|l| l.kind.severity().is_some())
        .count();
    format!(
        "{} nodes \u{00B7} {} links observed \u{00B7} {degraded} degraded, {strained} strained",
        spec::NODES.len(),
        spec::LINKS.len(),
    )
}

fn filter_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rail = filter_rect();
    let mut children = vec![
        part_box(
            "tv.filters.title",
            seat(rail.x + PAD, 18, rail.w.saturating_sub(PAD * 2), FONT_TITLE),
            vec![label(
                "Filters and layers",
                Rect::new(0, 0, rail.w.saturating_sub(PAD * 2), 0),
                FONT_TITLE,
                ink.text,
            )],
        ),
        part_box(
            "tv.filters.observed",
            seat(rail.x + PAD, 42, rail.w.saturating_sub(PAD * 2), FONT_SMALL),
            vec![label(
                observed_sentence(),
                Rect::new(0, 0, rail.w.saturating_sub(PAD * 2), 0),
                FONT_SMALL,
                ink.faint,
            )],
        ),
    ];
    children.push(group(
        "tv.filters.layout",
        0,
        "LAYOUT",
        ink,
        layout_group(state, ink),
    ));
    children.push(group(
        "tv.filters.links",
        1,
        "SHOW LINKS",
        ink,
        links_group(state, ink),
    ));
    children.push(group(
        "tv.filters.highlight",
        2,
        "HIGHLIGHT KEY PATTERN",
        ink,
        highlight_group(ink),
    ));
    children.push(group(
        "tv.filters.streaming",
        3,
        "STREAMING",
        ink,
        streaming_group(state, ink),
    ));
    panel(FILTER_TAG, rail, ink.surface, Some(ink.border), children)
}

/// One labelled group of the rail, as the specification's part.
fn group(tag: &str, nth: u32, heading: &str, ink: Ink, body: Vec<Scene>) -> Scene {
    let head = group_head_rect(nth);
    let whole = Rect::new(head.x, head.y, head.w, GROUP_HEAD_H + group_body_h(nth));
    let mut children = vec![label(
        heading.to_owned(),
        Rect::new(0, 0, head.w, GROUP_HEAD_H),
        FONT_TINY,
        ink.faint,
    )];
    children.extend(body);
    part_box(tag, whole, children)
}

fn layout_group(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let head = group_head_rect(0);
    let mut out = Vec::new();
    for (n, layout) in spec::LAYOUTS.iter().enumerate() {
        let nth = u32::try_from(n).unwrap_or(0);
        let rect = layout_rect(nth);
        let chosen = state.layout.get() == n;
        let local = Rect::new(rect.x - head.x, rect.y - head.y, rect.w, rect.h);
        out.push(captioned_box(
            &format!("tv.layout.{n}"),
            local,
            if chosen { ink.accent } else { ink.raised },
            Some(ink.edge),
            8,
            (
                layout.label(),
                FONT_BODY,
                if chosen { ink.surface } else { ink.dim },
            ),
            // Speaks for itself: an `AccessNode` names it and says whether it
            // is the layout in force.
            None,
        ));
    }
    out
}

fn links_group(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let head = group_head_rect(1);
    let mut out = Vec::new();
    for (n, toggle) in spec::TOGGLES
        .iter()
        .enumerate()
        .filter(|(_, t)| t.group == "links")
    {
        out.extend(toggle_row_scene(state, n, toggle, head, ink));
    }
    out
}

fn streaming_group(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let head = group_head_rect(3);
    let mut out = Vec::new();
    for (n, toggle) in spec::TOGGLES
        .iter()
        .enumerate()
        .filter(|(_, t)| t.group == "streaming")
    {
        out.extend(toggle_row_scene(state, n, toggle, head, ink));
    }
    out
}

/// One switch row, in its group's local coordinates.
fn toggle_row_scene(
    state: &Rc<ViewState>,
    nth: usize,
    toggle: &spec::ToggleSpec,
    head: Rect,
    ink: Ink,
) -> Vec<Scene> {
    let row = toggle_row(nth);
    let local = Rect::new(row.x - head.x, row.y - head.y, row.w, row.h);
    let on = state.toggle(nth);
    let track = switch_rect(local);
    vec![
        label(
            toggle.title,
            Rect::new(local.x, local.y + 9, local.w.saturating_sub(44), 14),
            FONT_BODY,
            ink.text,
        ),
        box_at(
            &format!("tv.toggle.{}", toggle.key),
            track,
            if on { ink.accent } else { ink.raised },
            Some(ink.edge),
            10,
            // A `Switch` access node names it and carries its on/off value.
            None,
        ),
        box_at(
            &format!("tv.toggle.{}.knob", toggle.key),
            Rect::new(
                if on { track.x + 20 } else { track.x + 2 },
                track.y + 2,
                16,
                16,
            ),
            ink.surface,
            None,
            8,
            Some(Silence::part_of(format!("tv.toggle.{}", toggle.key))),
        ),
    ]
}

fn highlight_group(ink: Ink) -> Vec<Scene> {
    let head = group_head_rect(2);
    let row = group_row_rect(2, 0);
    let local = Rect::new(row.x - head.x, row.y - head.y, row.w, row.h);
    let mut out = vec![captioned_box(
        "tv.highlight",
        local,
        ink.raised,
        Some(ink.border),
        8,
        (spec::HIGHLIGHT, FONT_BODY, ink.dim),
        Some(Silence::part_of("tv.filters.highlight")),
    )];
    for (n, chip) in spec::HIGHLIGHT_CHIPS.iter().enumerate() {
        let nth = u32::try_from(n).unwrap_or(0);
        let rect = chip_rect(nth);
        let chip_local = Rect::new(rect.x - head.x, rect.y - head.y, rect.w, rect.h);
        out.push(captioned_box(
            &format!("tv.chip.{n}"),
            chip_local,
            ink.raised,
            Some(ink.border),
            6,
            (chip, FONT_SMALL, ink.dim),
            // A `Button` access node names the pattern it highlights.
            None,
        ));
    }
    out
}

// ── The graph ───────────────────────────────────────────────────────────────

fn graph_pane(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let head = graph_header_rect();
    // ★★★★★ R1947 — **every header part is the full band tall, so the order a
    // reader reads them in is their order across the bar.**
    //
    // The first draft gave each part the height of its own run and placed them
    // at three different `y`s, one pixel apart; the conformance sweep then read
    // them in an order that was neither the reference's nor anything a person
    // would call an order, and reported four parts out of place. A part is a
    // REGION, and two regions in one band are ordered by where they are along
    // it — which is only true if they share the band.
    let band = |x: u32, w: u32| Rect::new(x, head.y, w, HEADER_H);
    let centred = |w: u32, px: u32| {
        pinion_core::containment::line_rect_in(Rect::new(0, 0, w, HEADER_H), 0, w, px)
    };
    let mut out = vec![
        panel(GRAPH_TAG, graph_rect(), ink.ground, None, Vec::new()),
        part_box(
            "tv.graph.title",
            band(head.x + 18, 150),
            vec![label(
                "Network topology",
                centred(150, FONT_TITLE),
                FONT_TITLE,
                ink.text,
            )],
        ),
        part_box(
            "tv.graph.layout_label",
            band(head.x + 180, 160),
            vec![label(
                format!("{} layout", state.layout().in_force()),
                centred(160, FONT_SMALL),
                FONT_SMALL,
                ink.faint,
            )],
        ),
    ];
    // ★ The live mark states the capture's state in WORDS in both directions.
    // The reference draws the mark only while capturing, so a stopped capture
    // is announced by an absence — which a reader who did not see it start
    // cannot read at all.
    let live = live_rect();
    out.push(part_box(
        "tv.graph.live",
        band(live.x, live.w),
        vec![label(
            if state.capturing() { "LIVE" } else { "PAUSED" },
            centred(live.w, FONT_SMALL),
            FONT_SMALL,
            if state.capturing() { ink.ok } else { ink.faint },
        )],
    ));
    let fit = fit_rect();
    out.push(part_box(
        "tv.graph.fit",
        band(fit.x, fit.w),
        vec![captioned_box(
            "tv.graph.fit.box",
            Rect::new(0, (HEADER_H - fit.h) / 2, fit.w, fit.h),
            ink.raised,
            Some(ink.edge),
            8,
            ("Fit", FONT_BODY, ink.dim),
            Some(Silence::part_of("tv.graph.fit")),
        )],
    ));
    out.push(canvas(state, ink));
    for (n, closer) in [(0_u32, true), (1_u32, false)] {
        let rect = zoom_rect(n);
        let tag = if closer {
            "tv.graph.zoom_in"
        } else {
            "tv.graph.zoom_out"
        };
        out.push(part_box(
            tag,
            rect,
            vec![captioned_box(
                &format!("{tag}.box"),
                Rect::new(0, 0, rect.w, rect.h),
                ink.surface,
                Some(ink.edge),
                8,
                (if closer { "+" } else { "\u{2212}" }, FONT_TITLE, ink.text),
                Some(Silence::part_of(tag)),
            )],
        ));
    }
    let hint = hint_rect();
    out.push(part_box(
        "tv.graph.hint",
        seat(hint.x, hint.y, hint.w, FONT_TINY),
        vec![label(
            "click a node to inspect",
            Rect::new(0, 0, hint.w, 0),
            FONT_TINY,
            ink.faint,
        )],
    ));
    out
}

/// The plot: links under nodes, each node reachable and named.
fn canvas(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let plot = canvas_rect();
    let (layout, zoom) = (state.layout(), state.zoom.get());
    let mut children = Vec::new();
    for (n, link) in spec::LINKS.iter().enumerate() {
        if !state.draws(link.kind) {
            continue;
        }
        let (Some(from), Some(to)) = (spec::node(link.from), spec::node(link.to)) else {
            continue;
        };
        let a = plot_point(from.at(layout), zoom);
        let b = plot_point(to.at(layout), zoom);
        let local = Rect::new(0, 0, plot.w, plot.h);
        let shift = |at: (i64, i64)| (at.0 - i64::from(plot.x), at.1 - i64::from(plot.y));
        children.push(line(
            local,
            shift(a),
            shift(b),
            link_ink(link.kind, ink),
            if link.kind == spec::LinkKind::Data {
                3
            } else {
                2
            },
        ));
        if let Some(text) = link.label {
            let mid = (
                i64::midpoint(shift(a).0, shift(b).0),
                i64::midpoint(shift(a).1, shift(b).1),
            );
            children.push(tagged_label(
                &format!("tv.link.{n}.label"),
                text,
                Rect::new(
                    u32::try_from(mid.0.max(0)).unwrap_or(0),
                    u32::try_from(mid.1.max(0)).unwrap_or(0),
                    46,
                    12,
                ),
                FONT_TINY,
                ink.faint,
                Silence::decorative(
                    "the rate this link carries; the inspector states it for the node",
                ),
            ));
        }
    }
    let picked = state.picked();
    for node in spec::NODES {
        let body = node_rect(node, layout, zoom);
        let local = Rect::new(
            body.x.saturating_sub(plot.x),
            body.y.saturating_sub(plot.y),
            body.w,
            body.h,
        );
        if node.id == picked.id {
            let ring = node_ring_rect(node, layout, zoom);
            children.push(box_at(
                &format!("tv.node.{}.ring", node.id),
                Rect::new(
                    ring.x.saturating_sub(plot.x),
                    ring.y.saturating_sub(plot.y),
                    ring.w,
                    ring.h,
                ),
                Color::rgba(0, 0, 0, 0),
                Some(ink.accent_fg),
                ring.h / 2,
                Some(Silence::decorative(
                    "marks the picked node; the node itself says it is selected",
                )),
            ));
        }
        // ★ The identifier is BOUND to the circle it names; the short word under
        // it is a second run and stays adjacent, because a box carries one
        // caption and the reference draws two lines here.
        children.push(captioned_box(
            &format!("tv.node.{}", node.id),
            local,
            ink.raised,
            Some(standing_ink(node.standing, ink)),
            if node.is_router() { 12 } else { local.h / 2 },
            (node.id, FONT_BODY, ink.text),
            // A `Button` access node names the peer and its role, and says
            // whether it is the one the inspector is describing.
            None,
        ));
        children.push(label(
            node.short,
            Rect::new(local.x, local.y + local.h / 2 + 2, local.w, 12),
            FONT_TINY,
            standing_ink(node.standing, ink),
        ));
    }
    part_box(CANVAS_TAG, plot, children)
}

// ── The inspector ───────────────────────────────────────────────────────────

fn inspector_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let pane = inspector_rect();
    let node = state.picked();
    let mut children = vec![
        part_box(
            "tv.inspector.title",
            seat(PAD, 18, 120, FONT_TITLE),
            vec![label(
                "Inspector",
                Rect::new(0, 0, 120, 0),
                FONT_TITLE,
                ink.text,
            )],
        ),
        part_box(
            "tv.inspector.badge",
            Rect::new(pane.w.saturating_sub(PAD + 82), 17, 82, 24),
            vec![captioned_box(
                "tv.inspector.badge.box",
                Rect::new(0, 0, 82, 24),
                ink.raised,
                Some(ink.border),
                6,
                (node.id, FONT_SMALL, ink.dim),
                Some(Silence::part_of("tv.inspector.badge")),
            )],
        ),
        part_box(
            "tv.inspector.id",
            seat(PAD, 66, pane.w.saturating_sub(PAD * 2), FONT_HEADLINE),
            vec![label(
                node.id,
                Rect::new(0, 0, pane.w.saturating_sub(PAD * 2), 0),
                FONT_HEADLINE,
                ink.text,
            )],
        ),
        part_box(
            "tv.inspector.status",
            Rect::new(PAD, 104, 118, 24),
            vec![captioned_box(
                "tv.inspector.status.pill",
                Rect::new(0, 0, 118, 24),
                ink.raised,
                Some(standing_ink(node.standing, ink)),
                12,
                (
                    node.standing.label(),
                    FONT_SMALL,
                    standing_ink(node.standing, ink),
                ),
                Some(Silence::part_of("tv.inspector.status")),
            )],
        ),
        part_box(
            "tv.inspector.role",
            seat(
                PAD + 128,
                107,
                pane.w.saturating_sub(PAD * 2 + 128),
                FONT_BODY,
            ),
            vec![label(
                node.role,
                Rect::new(0, 0, pane.w.saturating_sub(PAD * 2 + 128), 0),
                FONT_BODY,
                ink.dim,
            )],
        ),
        part_box(
            "tv.inspector.zid",
            seat(PAD, 136, pane.w.saturating_sub(PAD * 2), FONT_SMALL),
            vec![label(
                format!("session \u{00B7} {}", node.zid),
                Rect::new(0, 0, pane.w.saturating_sub(PAD * 2), 0),
                FONT_SMALL,
                ink.faint,
            )],
        ),
    ];
    children.extend(measurement_tiles(node, pane, ink));
    children.push(keys_band(node, pane, ink));
    children.extend(action_row(pane, ink));
    panel(INSPECTOR_TAG, pane, ink.surface, Some(ink.border), children)
}

/// The inspector's four measurements, in the two-by-two grid the reference
/// draws them in.
fn measurement_tiles(node: &spec::NodeSpec, pane: Rect, ink: Ink) -> Vec<Scene> {
    let tiles: [(&str, &str, String); 4] = [
        ("links", "LINKS", node.links.to_string()),
        ("rate", "MSG RATE", node.rate.to_owned()),
        ("encryption", "ENCRYPTION", node.encryption.to_owned()),
        ("state", "STATUS", node.standing.label().to_owned()),
    ];
    let mut out = Vec::new();
    for (n, (key, heading, value)) in tiles.into_iter().enumerate() {
        let nth = u32::try_from(n).unwrap_or(0);
        let rect = tile_rect(nth);
        let local = Rect::new(rect.x - pane.x, rect.y, rect.w, rect.h);
        out.push(part_box(
            &format!("tv.inspector.{key}"),
            local,
            vec![
                // ★ The VALUE is bound to the tile; the heading above it stays a
                // separate run, because the tile's box carries one caption and
                // the reference draws a label over a measurement.
                captioned_box(
                    &format!("tv.inspector.{key}.box"),
                    Rect::new(0, 0, local.w, local.h),
                    ink.raised,
                    Some(ink.border),
                    9,
                    (
                        &value,
                        FONT_BODY,
                        if key == "state" {
                            standing_ink(node.standing, ink)
                        } else {
                            ink.text
                        },
                    ),
                    Some(Silence::part_of(format!("tv.inspector.{key}"))),
                ),
                label(
                    heading,
                    Rect::new(12, 10, local.w.saturating_sub(24), 12),
                    FONT_TINY,
                    ink.faint,
                ),
            ],
        ));
    }
    out
}

/// The inspector's two actions, drawn and refused.
fn action_row(pane: Rect, ink: Ink) -> Vec<Scene> {
    let mut out = Vec::new();
    for (n, action) in spec::ACTIONS.iter().enumerate() {
        let nth = u32::try_from(n).unwrap_or(0);
        let rect = action_rect(nth);
        let local = Rect::new(rect.x - pane.x, rect.y, rect.w, rect.h);
        out.push(part_box(
            &format!("tv.inspector.{}", action.key),
            local,
            vec![captioned_box(
                &format!("tv.inspector.{}.box", action.key),
                Rect::new(0, 0, local.w, local.h),
                ink.raised,
                Some(ink.border),
                8,
                (action.title, FONT_BODY, ink.faint),
                Some(Silence::part_of(format!("tv.inspector.{}", action.key))),
            )],
        ));
    }
    out
}

/// The key patterns observed on the picked node.
///
/// ★ A node with none says so in words. An empty list is a legitimate state —
/// a node that is down declares nothing — and a blank space where a list would
/// be is indistinguishable from a list that failed to draw.
fn keys_band(node: &spec::NodeSpec, pane: Rect, ink: Ink) -> Scene {
    // ⚠ Every rectangle below is in the BAND's own coordinates, not the pane's.
    // The first draft mixed the two — the heading was placed at the window's
    // `y` inside a band that started there — and the sweep reported six marks
    // painted up to 270 pixels below the box that owns them. A part box is a
    // coordinate space, and a child that ignores that is drawn somewhere no
    // reader will look.
    let wide = pane.w.saturating_sub(PAD * 2);
    let mut children = vec![label(
        "KEY PATTERNS",
        Rect::new(PAD, 0, wide, 16),
        FONT_TINY,
        ink.faint,
    )];
    if node.keys.is_empty() {
        children.push(label(
            "No declarations observed",
            Rect::new(PAD, KEYS_ROW_TOP, wide, 16),
            FONT_BODY,
            ink.faint,
        ));
    } else {
        for (n, key) in node.keys.iter().enumerate() {
            let nth = u32::try_from(n).unwrap_or(0);
            let local = Rect::new(PAD, KEYS_ROW_TOP + nth * KEYS_ROW_PITCH, wide, 28);
            children.push(captioned_box(
                &format!("tv.inspector.keys.{n}"),
                local,
                ink.raised,
                Some(ink.border),
                7,
                (key, FONT_SMALL, ink.text),
                Some(Silence::part_of("tv.inspector.keys")),
            ));
        }
    }
    part_box(
        "tv.inspector.keys",
        Rect::new(1, KEYS_BAND_TOP, pane.w.saturating_sub(2), keys_band_h()),
        children,
    )
}

/// Where the key band starts in the inspector.
const KEYS_BAND_TOP: u32 = 318;
/// Where the first key row starts inside the band.
const KEYS_ROW_TOP: u32 = 26;
/// How far apart two key rows are.
const KEYS_ROW_PITCH: u32 = 34;

/// How tall the key band is — derived from the longest key list any node
/// declares, so the band holds its content at whichever node is picked rather
/// than at the one that happened to be open when the number was written.
fn keys_band_h() -> u32 {
    let rows = u32::try_from(
        spec::NODES
            .iter()
            .map(|n| n.keys.len())
            .max()
            .unwrap_or(1)
            .max(1),
    )
    .unwrap_or(1);
    KEYS_ROW_TOP + rows * KEYS_ROW_PITCH
}

// ── Descriptions ────────────────────────────────────────────────────────────

/// ★★★★★ R1947 — the sentences this screen's marks carry, by paint tag.
///
/// Derived from the declaration each mark is built from, never authored beside
/// it: a toggle's sentence comes from its own title and group, an action's from
/// the requirement that books it, and a node's from its standing and role. A
/// toggle added to [`spec::TOGGLES`] arrives described because the fields it is
/// described from are the ones that make it exist.
fn descriptions() -> Descriptions {
    let mut described = Descriptions::new();
    for toggle in spec::TOGGLES {
        described.describe(
            format!("tv.toggle.{}", toggle.key),
            format!("Show or hide {}", toggle.title.to_lowercase()),
        );
    }
    for (n, layout) in spec::LAYOUTS.iter().enumerate() {
        described.describe(
            format!("tv.layout.{n}"),
            format!("Arrange the plot in the {} layout", layout.in_force()),
        );
    }
    for action in spec::ACTIONS {
        described.describe(
            format!("tv.inspector.{}", action.key),
            format!(
                "{} is not in this release - booked under {}",
                action.title, action.reserved_for
            ),
        );
    }
    for node in spec::NODES {
        described.describe(
            format!("tv.node.{}", node.id),
            format!("{} - {}, {}", node.id, node.role, node.standing.label()),
        );
    }
    described.describe("tv.graph.fit", "Return the plot to the zoom it opened at");
    described.describe("tv.graph.zoom_in", "Draw the plot closer");
    described.describe("tv.graph.zoom_out", "Draw the plot further away");
    described
}

/// The description a reader is resting on, if any.
fn description_shown(state: &Rc<ViewState>) -> Option<(String, String)> {
    if !state.pointer_inside.get() {
        return None;
    }
    let tag = state.resting.get()?;
    let sentence = descriptions().of(&tag)?.to_owned();
    Some((tag, sentence))
}

/// Where a resting description is painted.
fn tip_rect() -> Rect {
    let plot = canvas_rect();
    Rect::new(plot.x + PAD, plot.y + PAD, 300, 22)
}

// ── The view ────────────────────────────────────────────────────────────────

fn view(_state: (), _frame: Frame) -> Scene {
    let state = use_view_state();
    let ink = ink();
    let (w, h) = window_size();
    let mut children = vec![filter_pane(&state, ink)];
    children.extend(graph_pane(&state, ink));
    children.push(inspector_pane(&state, ink));
    if let Some((_, sentence)) = description_shown(&state) {
        let tip = tip_rect();
        children.push(part_box(
            TOOLTIP_TAG,
            tip,
            vec![
                box_at(
                    "tv.tip.box",
                    Rect::new(0, 0, tip.w, tip.h),
                    ink.raised,
                    Some(ink.edge),
                    6,
                    Some(Silence::part_of(TOOLTIP_TAG)),
                ),
                label(
                    sentence,
                    Rect::new(9, 5, tip.w.saturating_sub(18), 12),
                    FONT_SMALL,
                    ink.text,
                ),
            ],
        ));
    }
    Scene::Container(
        ContainerNode::new(vec![
            panel(ROOT_TAG, Rect::new(0, 0, w, h), ink.ground, None, children).silenced(
                Silence::layout("places the filter rail, the graph column and the inspector"),
            ),
        ])
        .with_tag(VIEW_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(w, h))
                .with_silence(Silence::layout(
                    "the window's receiver; it holds the screen",
                )),
        ),
    )
}

// ── Accessibility ───────────────────────────────────────────────────────────

fn access_nodes(state: &Rc<ViewState>, focused: Option<&str>) -> Vec<AccessNode> {
    let described = descriptions();
    let mut nodes = vec![
        AccessNode::new(ROOT_TAG, AriaRole::Group)
            .with_name("Topology")
            .with_child(FILTER_TAG)
            .with_child(GRAPH_TAG)
            .with_child(INSPECTOR_TAG),
        AccessNode::new(FILTER_TAG, AriaRole::Group).with_name("Filters and layers"),
        AccessNode::new(GRAPH_TAG, AriaRole::Group).with_name("Network topology"),
        AccessNode::new(INSPECTOR_TAG, AriaRole::Group).with_name("Inspector"),
    ];
    // ★★★★★ R1947 — every part of every surface is named, from the SAME table
    // the pin is compared against. Not authored per part: a part the
    // specification gains arrives named, and a part named something other than
    // what the pin calls it is caught by
    // `tests::r1947_the_specified_parts_are_the_parts_this_build_tables`.
    for (stem, table) in [
        ("tv.filters", spec::FILTERS),
        ("tv.graph", spec::GRAPH),
        ("tv.inspector", spec::INSPECTOR),
    ] {
        for part in table {
            nodes.push(
                AccessNode::new(format!("{stem}.{}", part.key), AriaRole::Group)
                    .with_name(part.title),
            );
        }
    }
    for (n, layout) in spec::LAYOUTS.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("tv.layout.{n}"), AriaRole::RadioButton)
                .with_name(layout.label())
                .with_selected(state.layout.get() == n)
                .with_set_position(n, spec::LAYOUTS.len()),
        );
    }
    for (n, toggle) in spec::TOGGLES.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("tv.toggle.{}", toggle.key), AriaRole::Switch)
                .with_name(toggle.title)
                .with_value(AccessValue::Bool(state.toggle(n))),
        );
    }
    // ★ The highlight chips. Named from the pattern each one carries, which is
    // the only thing that tells two of them apart — a reader offered "chip" and
    // "chip" has been told nothing.
    for (n, chip) in spec::HIGHLIGHT_CHIPS.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("tv.chip.{n}"), AriaRole::Button)
                .with_name(format!("Highlight {chip}"))
                .with_set_position(n, spec::HIGHLIGHT_CHIPS.len()),
        );
    }
    let picked = state.picked();
    for node in spec::NODES {
        nodes.push(
            AccessNode::new(format!("tv.node.{}", node.id), AriaRole::Button)
                .with_name(format!("{} {}", node.id, node.role))
                .with_selected(node.id == picked.id),
        );
    }
    for action in spec::ACTIONS {
        nodes.push(
            AccessNode::new(format!("tv.inspector.{}", action.key), AriaRole::Button)
                .with_name(action.title)
                .with_state(AccessState {
                    disabled: true,
                    ..AccessState::default()
                }),
        );
    }
    nodes.push(AccessNode::new("tv.graph.fit", AriaRole::Button).with_name("Fit"));
    nodes.push(AccessNode::new("tv.graph.zoom_in", AriaRole::Button).with_name("Zoom in"));
    nodes.push(AccessNode::new("tv.graph.zoom_out", AriaRole::Button).with_name("Zoom out"));
    if let Some((tag, sentence)) = description_shown(state) {
        pinion_widget_paint::described::announce_description(
            &mut nodes,
            &tag,
            TOOLTIP_TAG,
            &sentence,
        );
    }
    let _ = (&described, focused);
    nodes
}

// ── The external ────────────────────────────────────────────────────────────

struct ViewOracle {
    state: Option<Rc<ViewState>>,
}

impl core::fmt::Debug for ViewOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ViewOracle")
            .field("attached", &self.state.is_some())
            .finish_non_exhaustive()
    }
}

impl ViewOracle {
    const fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: Rc<ViewState>) {
        self.state = Some(state);
    }
}

impl External for ViewOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn wants_hover_move(&self) -> bool {
        true
    }

    fn pointer_move(&mut self, at: PointerReading) {
        let Some(state) = self.state.clone() else {
            return;
        };
        let (px, py) = pinion_core::external::layout_point(VIEW_TAG, at.at);
        state.pointer_inside.set(true);
        state.resting.set(resting_tag(&state, px, py));
    }

    fn target_at(&self, x: u32, y: u32) -> PointerTarget {
        let (x, y) = pinion_core::external::into_layout(VIEW_TAG, (x, y));
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            Hit::at(s, x, y)
                .word()
                .map_or(PointerTarget::Nothing, PointerTarget::Word)
        })
    }

    fn target_of_tag(&self, tag: &str) -> PointerTarget {
        Hit::of_tag(tag)
            .word()
            .map_or(PointerTarget::Nothing, PointerTarget::Word)
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

/// Which described mark a point is resting on.
fn resting_tag(state: &Rc<ViewState>, px: u32, py: u32) -> Option<String> {
    match Hit::at(state, px, py) {
        Hit::Node(n) => Some(format!("tv.node.{}", spec::NODES[n].id)),
        Hit::Layout(n) => Some(format!("tv.layout.{n}")),
        Hit::Toggle(n) => Some(format!("tv.toggle.{}", spec::TOGGLES[n].key)),
        Hit::Action(n) => Some(format!("tv.inspector.{}", spec::ACTIONS[n].key)),
        Hit::Fit => Some("tv.graph.fit".to_owned()),
        Hit::Zoom(true) => Some("tv.graph.zoom_in".to_owned()),
        Hit::Zoom(false) => Some("tv.graph.zoom_out".to_owned()),
        Hit::Chip(_) | Hit::Nothing => None,
    }
}

/// What this section publishes about its own specification.
fn spec_json() -> serde_json::Value {
    serde_json::json!({
        "at": { "width": WIN_W, "height": WIN_H },
        "nodes": spec::NODES.len(),
        "links": spec::LINKS.len(),
        "layouts": spec::LAYOUTS.iter().map(|l| l.in_force()).collect::<Vec<_>>(),
    })
}

impl ExternalIntrospect for ViewOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("spec", "json"),
                    SchemaField::new("described", "json"),
                    SchemaField::new("conformance", "json"),
                    SchemaField::new("selected", "string"),
                    SchemaField::new("node", "json"),
                    SchemaField::new("layout", "string"),
                    SchemaField::new("zoom", "int"),
                    SchemaField::new("capturing", "bool"),
                    SchemaField::new("drawn_links", "int"),
                    SchemaField::new("said", "string"),
                    SchemaField::parametric(
                        "hit.<x>.<y>",
                        "string",
                        const { &[SchemaArg::open("x", "int"), SchemaArg::open("y", "int")] },
                    ),
                    SchemaField::action("select", "string"),
                    SchemaField::action("layout", "string"),
                    SchemaField::action("toggle", "string"),
                    SchemaField::action("zoom", "string"),
                    SchemaField::action("point", "string"),
                    SchemaField::action("press", "string"),
                    SchemaField::action("key", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("no capture is loaded"))?;
        if let Some(rest) = path.strip_prefix("hit.") {
            let (x, y) = rest.split_once('.').ok_or(ReadRefusal::QueryTypeMismatch)?;
            let (px, py) = (
                x.parse().map_err(|_| ReadRefusal::QueryTypeMismatch)?,
                y.parse().map_err(|_| ReadRefusal::QueryTypeMismatch)?,
            );
            return Ok(IntrospectValue::Text(
                Hit::at(state, px, py)
                    .word()
                    .unwrap_or_else(|| "none".to_owned()),
            ));
        }
        match path {
            "spec" => Ok(IntrospectValue::Json(spec_json())),
            "described" => Ok(IntrospectValue::Json(serde_json::json!(
                descriptions().tags().map(str::to_owned).collect::<Vec<_>>()
            ))),
            "conformance" => Ok(IntrospectValue::Json(
                serde_json::to_value(judge::conformance().to_json())
                    .unwrap_or(serde_json::Value::Null),
            )),
            "selected" => Ok(IntrospectValue::Text(state.selected.get())),
            "node" => {
                let node = state.picked();
                Ok(IntrospectValue::Json(serde_json::json!({
                    "id": node.id,
                    "role": node.role,
                    "standing": node.standing.label(),
                    "severity": node.standing.severity(),
                    "zid": node.zid,
                    "links": node.links,
                    "rate": node.rate,
                    "encryption": node.encryption,
                    "keys": node.keys,
                })))
            }
            "layout" => Ok(IntrospectValue::Text(state.layout().in_force().to_owned())),
            "zoom" => Ok(IntrospectValue::Int(i64::from(state.zoom.get()))),
            "capturing" => Ok(IntrospectValue::Bool(state.capturing())),
            "drawn_links" => Ok(IntrospectValue::Int(
                i64::try_from(spec::LINKS.iter().filter(|l| state.draws(l.kind)).count())
                    .unwrap_or(0),
            )),
            "said" => Ok(IntrospectValue::Text(state.said_sentence())),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    /// ★ Every refusal names what was refused and what the accepted values are.
    ///
    /// The verb answers nothing on success — the framework's contract — so what
    /// a driver reads afterwards is `said`, which is the same sentence a reader
    /// sees. One account of what happened rather than two.
    fn intervene(&mut self, path: &str, args: IntrospectValue) -> Result<(), InterveneError> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| InterveneError::out_of_range("the screen is not attached yet"))?
            .clone();
        let word = |args: &IntrospectValue| {
            args.as_str()
                .map(str::to_owned)
                .ok_or_else(|| InterveneError::out_of_range("expected a string argument"))
        };
        match path {
            "select" => {
                let id = word(&args)?;
                let nth = spec::NODES
                    .iter()
                    .position(|n| n.id == id)
                    .ok_or_else(|| InterveneError::out_of_range(format!("{id} is not a node")))?;
                select_node(&state, nth);
            }
            "layout" => {
                let name = word(&args)?;
                let nth = spec::LAYOUTS
                    .iter()
                    .position(|l| l.in_force() == name)
                    .ok_or_else(|| {
                        InterveneError::out_of_range(format!("{name} is not a layout"))
                    })?;
                choose_layout(&state, nth);
            }
            "toggle" => {
                let key = word(&args)?;
                let nth = spec::TOGGLES
                    .iter()
                    .position(|t| t.key == key)
                    .ok_or_else(|| {
                        InterveneError::out_of_range(format!("{key} is not a toggle"))
                    })?;
                flip_toggle(&state, nth);
            }
            "zoom" => match word(&args)?.as_str() {
                "in" => zoom_by(&state, true),
                "out" => zoom_by(&state, false),
                "fit" => fit(&state),
                other => {
                    return Err(InterveneError::out_of_range(format!(
                        "{other:?} is not a zoom; they are in / out / fit"
                    )));
                }
            },
            "press" => {
                let tag = word(&args)?;
                let hit = Hit::of_tag(&tag);
                if !press(&state, &hit) {
                    return Err(InterveneError::out_of_range(format!(
                        "{tag} is not pressable"
                    )));
                }
            }
            "point" => {
                let at = word(&args)?;
                let (x, y) = at
                    .split_once(',')
                    .ok_or_else(|| InterveneError::out_of_range("expected a point as \"x,y\""))?;
                let (px, py) = (
                    x.trim()
                        .parse::<u32>()
                        .map_err(|_| InterveneError::out_of_range("x is a whole number"))?,
                    y.trim()
                        .parse::<u32>()
                        .map_err(|_| InterveneError::out_of_range("y is a whole number"))?,
                );
                state.pointer_inside.set(true);
                state.resting.set(resting_tag(&state, px, py));
                let hit = Hit::at(&state, px, py);
                press(&state, &hit);
            }
            "key" => {
                let chord = word(&args)?;
                if !key_at(&state, &chord) {
                    return Err(InterveneError::out_of_range(format!(
                        "{chord} moves nothing on this screen"
                    )));
                }
            }
            _ => return Err(InterveneError::UnknownPath),
        }
        Ok(())
    }
}

// ── The binding ─────────────────────────────────────────────────────────────

/// ★ R1947 — public from the first round, because this screen is both a window
/// of its own and a **page** of the analysis-tool shell
/// (`pinion_screen::Mount<TopologyView>`).
pub struct TopologyView;

impl WidgetCore for TopologyView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ViewOracle::new();
        oracle.attach(use_view_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    /// ★★★★★ R1911 — this screen's marks are addressed under `tv.`, not under
    /// its root tag; the root is one marker node.
    fn paint_stems() -> Vec<&'static str> {
        vec![VIEW_TAG, "tv"]
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, *frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-topology-view (R1947 §5.41 topology section)"
    }

    /// ★★★★★ R1947 — **the arrows are this section's only while the focus is
    /// this section's.**
    ///
    /// The first draft ignored `focused` and answered every arrow. Mounted in
    /// the shell that is a section STEALING the host's keyboard: walking the
    /// rail with the arrow keys moved this plot's selection and left the rail's
    /// cursor where it was, so a person tabbing through the application stopped
    /// on the sixth seat and could not get past it. The shell's own roving
    /// test caught it on the first assembly.
    ///
    /// A screen in a window of its own reaches the same behaviour through
    /// `None` — nothing else is focusable there — so the standalone binary is
    /// unaffected, which is why this had to be a *test* rather than a look.
    fn apply_key(
        _scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let mine = focused.is_none_or(|tag| tag == VIEW_TAG || tag.starts_with("tv."));
        mine && key_at(&use_view_state(), chord)
    }
}

impl WidgetA11y for TopologyView {
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        access_nodes(&use_view_state(), focused)
    }

    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        let state = use_view_state();
        (focused == Some(CANVAS_TAG))
            .then(|| AccessFocus::composite(CANVAS_TAG, format!("tv.node.{}", state.picked().id)))
    }
}

impl WidgetView for TopologyView {
    type Renderer = HelloTopologyViewRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H))
    }

    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(SHRINK)
    }

    /// ★★★★★ R1738 — the same verdict this section publishes on its own wire,
    /// answered where a host assembling screens can reach it.
    fn conformance() -> Option<pinion_core::conformance::DocumentReport> {
        Some(judge::conformance())
    }
}

/// Run the topology section as an application of its own.
pub fn run() {
    pinion_shell::run::<TopologyView>();
}
