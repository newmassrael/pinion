// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-packet-view` — R1663 §5.27 §5.40 §5.41 — the analysis tool's
//! **capture viewer**, screen B, assembled as one application against a
//! written-down specification of the reference screen.
//!
//! ## What forced this example
//!
//! `docs/analyzer-census.json` carried `capture.t0.3` — *bidirectional
//! highlight between a field and its bytes* — as a `have`, covered by the hex
//! dump plus `scene/marks`. Driven on the wire, neither surface holds the
//! relation: the dissection External publishes thirteen paths and not one names
//! a byte; the hex External publishes seventeen and not one names a field. The
//! verdict was covering the byte↔*cell* pair, which is a different pair.
//!
//! [`pinion_core::widgets::field_bytes`] is the relation that was missing, and
//! this screen is the consumer that proves it composes: the decode tree, the
//! byte pane's highlight and the layer chain a reader sees are all derived from
//! **one** [`ByteMap`], so no two of them can disagree.
//!
//! ## The screen
//!
//! ```text
//! cargo run -p hello-packet-view --release
//! ```
//!
//! A filter bar over the always-visible session context, a three-pane body —
//! the message list, the layered decode, the bytes — and a reassembly strip
//! along the bottom. Click a message to decode it, click a decode row to light
//! its bytes, click a byte to select the field that owns it, click a layer
//! heading to fold it, click a saved filter to apply it.
//!
//! Every rectangle comes from the helpers above [`Hit::at`], read by both the
//! painter and the hit test, and the sweep in `painted.rs` presses the centre
//! of every painted control to keep it that way.
//!
//! See `tools/demos/r1663_a_field_says_which_bytes.py`.

mod spec;

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextOverflow, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::field_bytes::{
    ByteExtent, ByteMap, ByteMapExternal, ByteMapState, ByteSource, Coverage, FieldSpan, SourceId,
    use_byte_map,
};
use pinion_core::widgets::hex_dump::{ByteSelection, HexLayout};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloPacketViewRenderer, HelloPacketViewRendererError);

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = 1440;
const WIN_H: u32 = 900;
const VIEW_TAG: &str = "packet_view";
const MAP_TAG: &str = "pv.map";
const THEME_TAG: &str = "app";

const APP_BAR_H: u32 = spec::APP_BAR_H;
const FILTER_H: u32 = spec::FILTER_H;
const CONTEXT_H: u32 = spec::CONTEXT_H;
const REASSEMBLY_H: u32 = spec::REASSEMBLY_H;
const TREE_W: u32 = spec::PANES[1].width;
const BYTES_W: u32 = spec::PANES[2].width;

const PAD: u32 = 12;
const ROW_H: u32 = 22;
const HEAD_H: u32 = 24;
const FONT_TITLE: u32 = 14;
const FONT_SMALL: u32 = 11;
const FONT_MONO: u32 = 11;

/// The byte grid's cell size. Every column of the dump is a multiple of it, so
/// the painter and the hit test index the same lattice.
const CELL_W: u32 = 8;
const CELL_H: u32 = 18;

/// The width the specification's fixed columns need, summed from the
/// specification rather than copied out of it.
const fn fixed_columns() -> u32 {
    let mut total = 0;
    let mut i = 0;
    while i < spec::COLUMNS.len() {
        total += spec::COLUMNS[i].width;
        i += 1;
    }
    total
}

/// The narrowest the flexible `name` column may be and still show a resource
/// path rather than an ellipsis alone.
const NAME_FLOOR: u32 = 180;

/// The list pane's floor, **derived** from the columns it has to show.
///
/// ★ This was a round number (420) and the sweep's first run failed on it: the
/// fixed columns alone need more than that, so at the declared floor the
/// flexible column collapsed to zero width and its header stopped being
/// painted at all. A floor somebody picks is a floor that can be wrong about
/// the thing it is a floor FOR — the same shape as R1662's `MIN_H`, one level
/// down.
const LIST_FLOOR: u32 = fixed_columns() + NAME_FLOOR + PAD * 2;

/// The narrowest window this screen will lay out in.
///
/// ★ R1662's lesson is why this is a sum of the *panes* and not a number
/// somebody measured on a screen that happened to fit: a floor derived from
/// "the size at which nothing overflows" makes the sweep's smallest case the
/// case the defect chose.
const MIN_W: u32 = LIST_FLOOR + TREE_W + BYTES_W;
/// The shortest window this screen will lay out in — the chrome, plus room for
/// four message rows, plus the reassembly strip.
const MIN_H: u32 = APP_BAR_H + FILTER_H + CONTEXT_H + HEAD_H + ROW_H * 4 + REASSEMBLY_H;

/// The live surface, or the design size where no shell has published one.
///
/// `use_viewport_size` is a tracked read, so the view re-runs on a resize; it
/// is strict about the owner scope, and a bare unit call has none. The declared
/// design size is the honest fallback there — it is what the specification's
/// rectangles were measured against.
fn window_size() -> (u32, u32) {
    let live =
        pinion_core::reactive::Owner::current().map(|_| pinion_core::reactive::use_viewport_size());
    match live {
        Some((w, h)) if w >= MIN_W && h >= MIN_H => (w, h),
        _ => (WIN_W, WIN_H),
    }
}

fn body_rect() -> Rect {
    let (w, h) = window_size();
    let top = APP_BAR_H + FILTER_H + CONTEXT_H;
    Rect::new(0, top, w, h.saturating_sub(top + REASSEMBLY_H))
}

fn list_rect() -> Rect {
    let body = body_rect();
    Rect::new(
        body.x,
        body.y,
        body.w.saturating_sub(TREE_W + BYTES_W),
        body.h,
    )
}

fn tree_rect() -> Rect {
    let body = body_rect();
    Rect::new(list_rect().w, body.y, TREE_W, body.h)
}

fn bytes_rect() -> Rect {
    let body = body_rect();
    Rect::new(list_rect().w + TREE_W, body.y, BYTES_W, body.h)
}

fn filter_rect() -> Rect {
    Rect::new(0, APP_BAR_H, window_size().0, FILTER_H)
}

fn context_rect() -> Rect {
    Rect::new(0, APP_BAR_H + FILTER_H, window_size().0, CONTEXT_H)
}

fn reassembly_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(0, h - REASSEMBLY_H, w, REASSEMBLY_H)
}

// ── Ink ─────────────────────────────────────────────────────────────────────

const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// The inks this screen paints with, resolved from the theme once per frame.
#[derive(Clone, Copy)]
struct Ink {
    bg: Color,
    surface: Color,
    outline: Color,
    text: Color,
    text_2: Color,
    text_3: Color,
    accent: Color,
    warn: Color,
    err: Color,
    ok: Color,
    lit: Color,
}

fn ink(theme: &Theme) -> Ink {
    Ink {
        bg: rgb(0x0E_0F12),
        surface: rgb(0x16_181D),
        outline: rgb(0x2A_2E36),
        text: rgb(0xE8_EBEF),
        text_2: rgb(0x98_A2AD),
        text_3: rgb(0x69_7180),
        accent: rgb(0xEC_5AA0),
        warn: theme.resolve(ColorRole::Warning),
        err: theme.resolve(ColorRole::Error),
        ok: rgb(0x35_C08B),
        lit: Color::rgba(0x9A, 0x00, 0x4F, 0x60),
    }
}

/// The ink a message class is drawn in — the reader's index into a list going
/// past, which is why the reference gives five classes five colours.
fn kind_ink(kind: &str) -> Color {
    match kind {
        "Data" => rgb(0x3d_8b_fd),
        "Query" => rgb(0xb0_69_d8),
        "Response" => rgb(0x2e_a0_67),
        "Declare" => rgb(0xd1_8b_1f),
        _ => rgb(0x77_82_8c),
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// Everything the screen holds, and nothing it can derive.
struct ViewState {
    /// Which message of [`spec::ROWS`] is decoded.
    row: Signal<usize>,
    /// Which decode field is selected, by path.
    field: Signal<String>,
    /// Which saved filters are on.
    saved: Signal<Vec<bool>>,
    /// Which layers are folded, by index into [`spec::LAYERS`].
    folded: Signal<Vec<bool>>,
    /// The dissection of the selected message. **One** value: the tree, the
    /// byte highlight and the layer chain all read it.
    map: Rc<ByteMapState>,
    /// The scroll offsets of the three pane bodies.
    list_scroll: Rc<ScrollState>,
    tree_scroll: Rc<ScrollState>,
    bytes_scroll: Rc<ScrollState>,
    /// Where the cursor last was, because a press carries no coordinates.
    cursor: Signal<(u32, u32)>,
    /// The last thing the screen did, for the status line and the wire.
    said: RefCell<String>,
}

impl ViewState {
    fn say(&self, what: impl Into<String>) {
        *self.said.borrow_mut() = what.into();
    }

    /// The bytes of the frame this screen shows. Deterministic and derived from
    /// the selected row, so a decode and its dump cannot come from different
    /// messages.
    fn frame_bytes(&self) -> Vec<u8> {
        frame_bytes(self.row.get())
    }

    /// The selected field's highlight over the frame, when it has one there.
    fn lit_selection(&self) -> Option<ByteSelection> {
        let map = self.map.map();
        match map.selection_for(&self.field.get()) {
            Ok((source, sel)) if source == SourceId::new(0) => Some(sel),
            _ => None,
        }
    }
}

/// The frame bytes of message `row` — a stable pseudo-capture, so the dump is
/// the same on every host and in every run.
fn frame_bytes(row: usize) -> Vec<u8> {
    let len = spec::SOURCES[0].1;
    let seed = u32::try_from(row).unwrap_or(0).wrapping_mul(0x9e37) ^ 0x5a5a;
    (0..len)
        .map(|i| {
            let n = u32::try_from(i).unwrap_or(0);
            ((seed.wrapping_add(n.wrapping_mul(31)) >> 3) & 0xff) as u8
        })
        .collect()
}

fn use_view_state() -> Rc<ViewState> {
    // ★ [[owner-cache-no-nested-factory]] — every cached slot this one holds is
    // resolved BEFORE the factory runs, because `Owner::cache` cannot re-enter
    // itself and a factory that calls another `use_*` hook does exactly that.
    let map = use_byte_map("packet_view.map", || decode(spec::OPENING_ROW));
    let list_scroll = pinion_core::widgets::scroll::use_scroll_state("pv.list.body");
    let tree_scroll = pinion_core::widgets::scroll::use_scroll_state("pv.tree.body");
    let bytes_scroll = pinion_core::widgets::scroll::use_scroll_state("pv.bytes.body");
    let owner = pinion_core::reactive::Owner::current()
        .expect("use_view_state requires an active Owner scope");
    owner.cache("packet_view.state", || ViewState {
        row: Signal::new(spec::OPENING_ROW),
        field: Signal::new(spec::OPENING_FIELD.to_owned()),
        saved: Signal::new(vec![false; spec::SAVED_FILTERS.len()]),
        folded: Signal::new(vec![false; spec::LAYERS.len()]),
        map,
        list_scroll,
        tree_scroll,
        bytes_scroll,
        cursor: Signal::new((0, 0)),
        said: RefCell::new(String::new()),
    })
}

/// The example's decoder: the dissection of message `row`.
///
/// The reference states the decode of one message in full, and that table is
/// [`spec::FIELDS`] — reproduced verbatim for that row. Every other message
/// gets the same three layers built from the facts the row itself carries, so
/// selecting a different message produces a *real* dissection of a real length
/// rather than a screen that goes blank.
///
/// A row is reassembled when the specification says it is, and only then does
/// the map have a second source — which is the case the crate's source id
/// exists for.
fn decode(row: usize) -> ByteMap {
    if row == spec::OPENING_ROW {
        return spec_map();
    }
    let message = &spec::ROWS[row];
    let frame = SourceId::new(0);
    let frame_len = spec::SOURCES[0].1;
    // The message's own length decides how much of the frame the message layer
    // covers; the framing and transport headers are fixed.
    let body = (message.len as usize).clamp(4, frame_len - 0x14);
    let spans = vec![
        FieldSpan::bytes("l0", frame, ByteExtent::new(0x00, 0x0c)),
        FieldSpan::bytes("l0.link", frame, ByteExtent::new(0x00, 0x06)),
        FieldSpan::bytes("l0.stream", frame, ByteExtent::new(0x06, 0x06)),
        FieldSpan::bytes("l1", frame, ByteExtent::new(0x0c, 0x08)),
        FieldSpan::bytes("l1.sn", frame, ByteExtent::new(0x0e, 0x02)),
        FieldSpan::bytes("l3", frame, ByteExtent::new(0x14, body)),
        FieldSpan::bytes("l3.name_id", frame, ByteExtent::new(0x14, 0x02)),
        FieldSpan::derived("l3.resolved"),
    ];
    ByteMap::build(vec![ByteSource::new(spec::SOURCES[0].0, frame_len)], spans)
        .expect("the example's decoder produces a well-formed dissection")
}

/// The reference's own decode table, as a [`ByteMap`].
fn spec_map() -> ByteMap {
    let sources = spec::SOURCES
        .iter()
        .map(|(name, len)| ByteSource::new(*name, *len))
        .collect();
    let spans = spec::FIELDS
        .iter()
        .map(|f| match f.source {
            Some(s) => FieldSpan::bytes(f.path, SourceId::new(s), ByteExtent::new(f.at, f.len)),
            None => FieldSpan::derived(f.path),
        })
        .collect();
    ByteMap::build(sources, spans)
        .expect("the reference's decode table is a well-formed dissection")
}

/// The decode rows on screen: the specification's rows for the reference
/// message, and the decoder's own paths for every other one, minus whatever a
/// folded layer hides.
fn visible_fields(state: &ViewState) -> Vec<(String, String, String, usize)> {
    let map = state.map.map();
    let folded = state.folded.get();
    let described = state.row.get() == spec::OPENING_ROW;
    map.fields()
        .iter()
        .filter_map(|span| {
            let path = span.path().to_owned();
            let layer = spec::LAYERS.iter().position(|(id, _)| path.starts_with(id));
            if let Some(n) = layer
                && folded.get(n).copied().unwrap_or(false)
                && path.contains('.')
            {
                return None;
            }
            let (name, value) = if described {
                spec::FIELDS
                    .iter()
                    .find(|f| f.path == path)
                    .map_or((path.clone(), String::new()), |f| {
                        (f.name.to_owned(), f.value.to_owned())
                    })
            } else {
                let leaf = path.rsplit('.').next().unwrap_or(&path).to_owned();
                let value = map.extent_of(&path).map_or_else(
                    || "derived".to_owned(),
                    |(_, e)| format!("{} B at 0x{:02x}", e.len(), e.at()),
                );
                (leaf, value)
            };
            let depth = usize::from(path.contains('.'));
            Some((path, name, value, depth))
        })
        .collect()
}

// ── Paint helpers ───────────────────────────────────────────────────────────

/// ★★★★★ R1664 — and the pointer declaration is load-bearing, exactly as it is
/// on this screen's two siblings.
///
/// This screen routes every press to one root `External` and addresses its parts
/// by tag. Those two facts fight: the §5.35 router resolves the **deepest tagged
/// node** under the cursor and looks its primary half up as an `External`, so
/// every tag painted for addressing shadows the root and the press is dropped
/// without a word. `scene/click {path}`, `scene/invoke` and `send` all keep
/// working, because they call the handler by name and never ask the router —
/// which is why R1663 shipped this screen with 11 integration tests, a
/// 160-assertion demo and a boot gate all green, and a person opening the window
/// found that pressing anything did nothing at all.
///
/// `hello-node-lab` and `hello-analyzer-shell` both carry this line, and
/// `hello-analyzer-shell` carries R1649.1's account of learning it. This file
/// was written after both and without it.
///
/// The root container is the one node that keeps its own layout, so it stays the
/// target; everything built through this helper is an address.
fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

/// The style every run on this screen carries.
///
/// ★ R1654 — including an overflow policy. A resource name, a hop and a
/// timestamp are all captured data, so a run wider than the box it was given
/// would wrap onto the row below. `Ellipsis` rather than `Clip`, because a hard
/// cut leaves no evidence that anything was removed.
fn run_style(px: u32, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(px)
        .with_fg(fg)
        .with_overflow(TextOverflow::Ellipsis)
}

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

/// A bordered panel's CONTENT rectangle in its own space: its box less the 1px
/// frame [`panel`] draws inside it.
///
/// ★ R1672 — the placement half of
/// [`pinion_core::containment::content_rect`], which is the check half. A pane
/// that handed its scrolling body `(0, 0, rect.w, rect.h)` put the body over
/// its own outline, and the channel could not say so until it learned the
/// border-box / content-box distinction. Named here so the two halves cannot
/// drift: change the frame's width and both follow.
/// The width of the outline [`panel`] strokes INSIDE its box.
const PANEL_FRAME: u32 = 1;

fn panel_content(rect: Rect) -> Rect {
    pinion_core::containment::content_of(
        Rect::new(0, 0, rect.w, rect.h),
        Some(&Border::new(Color::rgba(0, 0, 0, 0), PANEL_FRAME)),
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
        style = style.with_border(Border::new(colour, PANEL_FRAME));
    }
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag.to_owned())
            .with_style(style)
            .with_layout(absolute(rect)),
    )
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

/// The centre of a rectangle — where the sweep presses, because a control that
/// does not answer at the middle of its own paint is not reachable.
#[cfg(test)]
const fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

/// A window point in a scrolling pane's own coordinates.
///
/// ★ R1662 — the pane's origin and its scroll offset are folded in ONE place,
/// because a screen that folds them per call site ends up with a painter that
/// moved and a hit test that did not.
fn in_pane(scroll: &ScrollState, pane: Rect, px: u32, py: u32) -> (u32, u32) {
    let (ox, oy) = scroll.offset();
    let fold = |v: u32, origin: u32, by: i32| -> u32 {
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "clamped into u32's range on the line above the cast"
        )]
        let folded =
            (i64::from(v) - i64::from(origin) + i64::from(by)).clamp(0, i64::from(u32::MAX)) as u32;
        folded
    };
    (fold(px, pane.x, ox), fold(py, pane.y, oy))
}

// ── The rectangles both the painter and the hit test read ───────────────────

/// The n-th saved-filter chip, in window coordinates.
fn saved_chip(n: usize) -> Rect {
    let width = 116;
    let x = filter_rect().w.saturating_sub(
        320 + u32::try_from(spec::SAVED_FILTERS.len() - n).unwrap_or(1) * (width + 8),
    );
    Rect::new(x, APP_BAR_H + 12, width, 22)
}

/// The n-th message row, in the list pane's own coordinates.
fn list_row(n: usize) -> Rect {
    Rect::new(
        0,
        HEAD_H + u32::try_from(n).unwrap_or(0) * ROW_H,
        list_rect().w,
        ROW_H,
    )
}

/// The n-th column of the message list, in the list pane's own coordinates.
fn list_col(n: usize) -> Rect {
    let flex = list_rect()
        .w
        .saturating_sub(spec::COLUMNS.iter().map(|c| c.width).sum::<u32>() + PAD * 2);
    let mut x = PAD;
    for (i, column) in spec::COLUMNS.iter().enumerate() {
        let w = if column.width == 0 {
            flex
        } else {
            column.width
        };
        if i == n {
            return Rect::new(x, 0, w, ROW_H);
        }
        x += w;
    }
    Rect::new(x, 0, 0, ROW_H)
}

/// The n-th decode row, in the tree pane's own coordinates.
fn tree_row(n: usize) -> Rect {
    Rect::new(
        0,
        HEAD_H + u32::try_from(n).unwrap_or(0) * ROW_H,
        TREE_W,
        ROW_H,
    )
}

/// The byte grid's column arithmetic — the crate's, so the painter and the hit
/// test cannot each derive their own.
fn hex_layout() -> HexLayout {
    HexLayout::new(spec::SOURCES[0].1).with_bytes_per_row(spec::BYTES_PER_ROW)
}

/// The n-th byte's hex cell, in the byte pane's own coordinates.
fn byte_cell(byte: usize) -> Option<Rect> {
    let layout = hex_layout();
    let cell = layout.hex_cell(byte)?;
    Some(Rect::new(
        PAD + u32::try_from(cell.col).unwrap_or(0) * CELL_W,
        HEAD_H + u32::try_from(cell.row).unwrap_or(0) * CELL_H,
        CELL_W * 2,
        CELL_H,
    ))
}

/// The n-th reassembly lane.
fn lane_rect(n: usize) -> Rect {
    let strip = reassembly_rect();
    let w =
        (strip.w.saturating_sub(PAD * 2)) / u32::try_from(spec::LANES.len().max(1)).unwrap_or(1);
    Rect::new(
        strip.x + PAD + u32::try_from(n).unwrap_or(0) * w,
        strip.y + 34,
        w.saturating_sub(10),
        44,
    )
}

// ── The hit test ────────────────────────────────────────────────────────────

/// What is under a point. One enum, resolved from the same rectangles the
/// painter uses.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// A message row.
    Message(usize),
    /// A decode row, by its path.
    Field(String),
    /// A byte of the frame.
    Byte(usize),
    /// A saved-filter chip.
    Saved(usize),
    /// A layer heading, which folds it.
    Layer(usize),
    /// Nothing that answers.
    None,
}

impl Hit {
    /// What answers at the window point `(px, py)`.
    fn at(state: &ViewState, px: u32, py: u32) -> Self {
        for (n, _) in spec::SAVED_FILTERS.iter().enumerate() {
            if contains(saved_chip(n), px, py) {
                return Self::Saved(n);
            }
        }
        let list = list_rect();
        if contains(list, px, py) {
            let (lx, ly) = in_pane(&state.list_scroll, list, px, py);
            for n in 0..spec::ROWS.len() {
                if contains(list_row(n), lx, ly) {
                    return Self::Message(n);
                }
            }
            return Self::None;
        }
        let tree = tree_rect();
        if contains(tree, px, py) {
            let (tx, ty) = in_pane(&state.tree_scroll, tree, px, py);
            for (n, (path, ..)) in visible_fields(state).into_iter().enumerate() {
                if contains(tree_row(n), tx, ty) {
                    if let Some(layer) =
                        spec::LAYERS.iter().position(|(id, _)| *id == path.as_str())
                    {
                        return Self::Layer(layer);
                    }
                    return Self::Field(path);
                }
            }
            return Self::None;
        }
        let bytes = bytes_rect();
        if contains(bytes, px, py) {
            let (bx, by) = in_pane(&state.bytes_scroll, bytes, px, py);
            for byte in 0..spec::SOURCES[0].1 {
                if byte_cell(byte).is_some_and(|r| contains(r, bx, by)) {
                    return Self::Byte(byte);
                }
            }
        }
        Self::None
    }
}

// ── The handlers a press and the wire both reach ────────────────────────────

fn select_message(state: &Rc<ViewState>, row: usize) {
    if row >= spec::ROWS.len() {
        return;
    }
    state.row.set(row);
    state.map.set(decode(row));
    // The previously selected field may not exist in the new decode. Fall back
    // to the outermost layer rather than to nothing, because a decode pane with
    // no selection has no highlight to show and the screen would look broken.
    let map = state.map.map();
    if map.field(&state.field.get()).is_none() {
        state.field.set(spec::LAYERS[0].0.to_owned());
    }
    state.say(format!("message {row}"));
}

fn select_field(state: &Rc<ViewState>, path: &str) {
    if state.map.map().field(path).is_none() {
        return;
    }
    state.field.set(path.to_owned());
    state.say(format!("field {path}"));
}

/// The inverse direction as a gesture: a press on a byte selects the field that
/// owns it. The screen does not keep its own byte→field table — the map is
/// asked, so the highlight a press produces and the highlight a field selection
/// produces are the same derivation.
fn select_byte(state: &Rc<ViewState>, byte: usize) {
    let map = state.map.map();
    match map.coverage_at(SourceId::new(0), byte) {
        Coverage::Field(span) => {
            let path = span.path().to_owned();
            state.field.set(path.clone());
            state.say(format!("byte {byte} is {path}"));
        }
        Coverage::Unmapped => state.say(format!("byte {byte} is claimed by no field")),
        Coverage::OutOfBuffer => state.say(format!("byte {byte} is past the frame")),
    }
}

fn toggle_saved(state: &Rc<ViewState>, n: usize) {
    let mut saved = state.saved.get();
    if let Some(slot) = saved.get_mut(n) {
        *slot = !*slot;
        let on = *slot;
        state.saved.set(saved);
        state.say(format!(
            "{} {}",
            if on { "applied" } else { "cleared" },
            spec::SAVED_FILTERS[n]
        ));
    }
}

fn toggle_layer(state: &Rc<ViewState>, n: usize) {
    let mut folded = state.folded.get();
    if let Some(slot) = folded.get_mut(n) {
        *slot = !*slot;
        state.folded.set(folded);
        state.say(format!("layer {}", spec::LAYERS[n].0));
    }
}

fn move_cursor(state: &Rc<ViewState>, px: u32, py: u32) {
    state.cursor.set((px, py));
}

fn press(state: &Rc<ViewState>) {
    let (px, py) = state.cursor.get();
    match Hit::at(state, px, py) {
        Hit::Message(n) => select_message(state, n),
        Hit::Field(path) => select_field(state, &path),
        Hit::Byte(b) => select_byte(state, b),
        Hit::Saved(n) => toggle_saved(state, n),
        Hit::Layer(n) => toggle_layer(state, n),
        Hit::None => {}
    }
}

fn key(state: &Rc<ViewState>, chord: &str) -> bool {
    match chord {
        "Down" | "Up" => {
            let row = state.row.get();
            let next = if chord == "Down" {
                (row + 1).min(spec::ROWS.len() - 1)
            } else {
                row.saturating_sub(1)
            };
            select_message(state, next);
            true
        }
        "Escape" => {
            state.field.set(spec::LAYERS[0].0.to_owned());
            true
        }
        _ => false,
    }
}

// ── The view ────────────────────────────────────────────────────────────────

fn view(_state: (), _frame: Frame) -> Scene {
    let state = use_view_state();
    let theme = use_theme(THEME_TAG).theme_animated();
    let ink = ink(&theme);
    let (w, h) = window_size();
    Scene::Container(
        ContainerNode::new(vec![panel(
            "pv.root",
            Rect::new(0, 0, w, h),
            ink.bg,
            None,
            vec![
                app_bar(&state, ink),
                filter_bar(&state, ink),
                context_strip(ink),
                list_pane(&state, ink),
                tree_pane(&state, ink),
                bytes_pane(&state, ink),
                reassembly_strip(ink),
            ],
        )])
        // ★ R1664 — the root carries the tag the widget is REGISTERED under, so
        // the router has something to resolve a press to. `pv.root` above is an
        // ADDRESS, for `scene/snapshot` and the sweep; this is the RECEIVER.
        // They were two string literals in two functions with nothing checking
        // that either of them named anything, and the screen was dead at every
        // point in the window. `scene/pointer_reach`.externals is the read that
        // now holds both sides of that join.
        .with_tag(VIEW_TAG)
        .with_layout(LayoutStyle::new().with_size(Size::px(w, h))),
    )
}

fn app_bar(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let (w, _) = window_size();
    panel(
        "pv.appbar",
        Rect::new(0, 0, w, APP_BAR_H),
        ink.surface,
        Some(ink.outline),
        vec![
            label(
                "packet view",
                Rect::new(16, 19, 96, 16),
                FONT_TITLE,
                ink.text,
            ),
            tagged_label(
                "pv.appbar.interface",
                spec::INTERFACE,
                Rect::new(124, 20, 190, 14),
                FONT_SMALL,
                ink.text_2,
            ),
            tagged_label(
                "pv.appbar.rate",
                spec::RATE,
                Rect::new(324, 20, 120, 14),
                FONT_SMALL,
                ink.ok,
            ),
            tagged_label(
                "pv.appbar.said",
                state.said.borrow().clone(),
                Rect::new(w.saturating_sub(360), 20, 340, 14),
                FONT_SMALL,
                ink.text_3,
            ),
        ],
    )
}

fn filter_bar(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = filter_rect();
    let saved = state.saved.get();
    let mut children = Vec::new();
    let mut x = PAD;
    for (n, clause) in spec::QUERY_CLAUSES.iter().enumerate() {
        let width = u32::try_from(clause.len()).unwrap_or(20) * 7 + 12;
        children.push(tagged_label(
            &format!("pv.filter.clause.{n}"),
            *clause,
            Rect::new(x, 16, width, 14),
            FONT_SMALL,
            if n == 0 { ink.text } else { ink.text_2 },
        ));
        x += width + 6;
    }
    for (n, name) in spec::SAVED_FILTERS.iter().enumerate() {
        let chip = saved_chip(n);
        let on = saved.get(n).copied().unwrap_or(false);
        children.push(box_at(
            &format!("pv.filter.saved.{n}"),
            Rect::new(chip.x, chip.y - rect.y, chip.w, chip.h),
            if on { ink.lit } else { ink.surface },
            Some(if on { ink.accent } else { ink.outline }),
            11,
        ));
        children.push(label(
            *name,
            Rect::new(chip.x + 10, chip.y - rect.y + 5, chip.w - 20, 12),
            FONT_SMALL,
            if on { ink.accent } else { ink.text_2 },
        ));
    }
    children.push(tagged_label(
        "pv.filter.count",
        format!("{} / {}", comma(spec::MATCHED), comma(spec::CAPTURED)),
        Rect::new(rect.w.saturating_sub(196), 16, 180, 14),
        FONT_SMALL,
        ink.text_2,
    ));
    panel("pv.filter", rect, ink.surface, Some(ink.outline), children)
}

/// A count with thousands separators, the way the reference prints one.
fn comma(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The box a small run of `text` needs, derived from its own length.
///
/// ★ The context strip's first layout gave every part a fixed slot and put the
/// consequence note at a fixed offset inside it; the sweep's first run caught
/// `off` and `4 layers` painted on top of each other. A cursor that advances by
/// what it just placed cannot produce that, whatever the content turns out to
/// be — the fixed slot was a number that had to stay true about strings nobody
/// had measured.
fn run_box(text: &str, x: u32, y: u32) -> Rect {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a strip label is a handful of characters"
    )]
    let chars = text.chars().count() as u32;
    Rect::new(x, y, chars * (FONT_SMALL - 4) + 10, 14)
}

fn context_strip(ink: Ink) -> Scene {
    let rect = context_rect();
    let session = format!("negotiated · session {}", spec::SESSION);
    let session_box = run_box(&session, PAD, 12);
    let mut children = vec![tagged_label(
        "pv.context.session",
        session.clone(),
        session_box,
        FONT_SMALL,
        ink.text_3,
    )];
    let mut x = session_box.x + session_box.w + 24;
    for value in spec::CONTEXT {
        let slug = value.key.replace(' ', "_");
        let key_box = run_box(value.key, x, 12);
        children.push(label(value.key, key_box, FONT_SMALL, ink.text_3));
        x = key_box.x + key_box.w;
        let value_box = run_box(value.value, x, 12);
        children.push(tagged_label(
            &format!("pv.context.{slug}"),
            value.value,
            value_box,
            FONT_SMALL,
            ink.text,
        ));
        x = value_box.x + value_box.w;
        if !value.note.is_empty() {
            let note_box = run_box(value.note, x, 12);
            children.push(label(value.note, note_box, FONT_SMALL, ink.text_3));
            x = note_box.x + note_box.w;
        }
        x += 18;
    }
    panel("pv.context", rect, ink.bg, Some(ink.outline), children)
}

fn list_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = list_rect();
    let selected = state.row.get();
    let mut children = Vec::new();
    for (n, column) in spec::COLUMNS.iter().enumerate() {
        let col = list_col(n);
        children.push(tagged_label(
            &format!("pv.list.head.{n}"),
            column.title,
            Rect::new(col.x, 6, col.w.saturating_sub(8), 12),
            FONT_SMALL,
            ink.text_3,
        ));
    }
    for n in 0..spec::ROWS.len() {
        children.extend(list_row_paint(n, selected, ink));
    }
    panel(
        "pv.list",
        rect,
        ink.bg,
        Some(ink.outline),
        vec![scroll_pane(
            &state.list_scroll,
            panel_content(rect),
            (0, PAD),
            PanePointer::PassesThrough,
            children,
        )],
    )
}

/// One message row, in the list pane's own coordinates.
fn list_row_paint(n: usize, selected: usize, ink: Ink) -> Vec<Scene> {
    let message = &spec::ROWS[n];
    let mut children = Vec::new();
    {
        let row = list_row(n);
        if n == selected {
            children.push(box_at(
                "pv.list.selected",
                row,
                ink.lit,
                Some(ink.accent),
                0,
            ));
        }
        children.push(box_at(
            &format!("pv.list.row.{n}"),
            Rect::new(row.x, row.y, row.w, row.h),
            Color::rgba(0, 0, 0, 0),
            None,
            0,
        ));
        let cell = |i: usize| {
            let c = list_col(i);
            Rect::new(c.x, row.y + 5, c.w.saturating_sub(8), 12)
        };
        let text = ink.text;
        children.push(label(message.time, cell(0), FONT_SMALL, ink.text_2));
        children.push(label(message.hop, cell(1), FONT_SMALL, text));
        children.push(label(message.channel, cell(2), FONT_SMALL, ink.text_2));
        children.push(label(
            message.sn.to_string(),
            cell(3),
            FONT_SMALL,
            ink.text_2,
        ));
        children.push(tagged_label(
            &format!("pv.list.row.{n}.kind"),
            message.kind,
            cell(4),
            FONT_SMALL,
            kind_ink(message.kind),
        ));
        // ★ The name column is shared with the row's annotations, so the
        // annotations are placed FIRST, from the right edge inward, and the
        // name takes what is left. The first draft gave the name the whole
        // column and put the annotations at fixed offsets inside it; the
        // sweep's first run found `piece 1 of 3` painted underneath `First
        // 1/3`. Subtracting what is already placed cannot produce that,
        // whatever the strings turn out to be.
        let name_col = cell(5);
        let mut right = name_col.x + name_col.w;
        if !message.note.is_empty() {
            let width = run_box(message.note, 0, 0).w;
            right = right.saturating_sub(width);
            children.push(tagged_label(
                &format!("pv.list.row.{n}.note"),
                message.note,
                Rect::new(right, row.y + 5, width, 12),
                FONT_SMALL,
                ink.warn,
            ));
        }
        if let Some(fragment) = &message.fragment {
            let marker = format!("{} {}", fragment.marker, fragment.piece);
            let width = run_box(&marker, 0, 0).w;
            right = right.saturating_sub(width + 8);
            children.push(tagged_label(
                &format!("pv.list.row.{n}.fragment"),
                marker,
                Rect::new(right, row.y + 5, width, 12),
                FONT_SMALL,
                if fragment.marker == "Drop" {
                    ink.err
                } else {
                    ink.warn
                },
            ));
        }
        children.push(label(
            message.name,
            Rect::new(
                name_col.x,
                row.y + 5,
                right.saturating_sub(name_col.x + 8),
                12,
            ),
            FONT_SMALL,
            text,
        ));
        children.push(label(
            message.len.to_string(),
            cell(6),
            FONT_SMALL,
            ink.text_2,
        ));
    }
    children
}

fn tree_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = tree_rect();
    let selected = state.field.get();
    let folded = state.folded.get();
    let map = state.map.map();
    let mut children = vec![tagged_label(
        "pv.tree.title",
        format!("{}  ·  L0 -> L3", spec::PANES[1].title),
        Rect::new(PAD, 6, 200, 12),
        FONT_SMALL,
        ink.text_3,
    )];
    for (n, (path, name, value, depth)) in visible_fields(state).into_iter().enumerate() {
        let row = tree_row(n);
        let layer = spec::LAYERS.iter().position(|(id, _)| *id == path.as_str());
        if path == selected {
            children.push(box_at(
                "pv.tree.selected",
                row,
                ink.lit,
                Some(ink.accent),
                0,
            ));
        }
        let indent = PAD + u32::try_from(depth).unwrap_or(0) * 14;
        if let Some(index) = layer {
            children.push(tagged_label(
                &format!("pv.tree.layer.{}", spec::LAYERS[index].0),
                if folded.get(index).copied().unwrap_or(false) {
                    ">"
                } else {
                    "v"
                },
                Rect::new(PAD - 6, row.y + 5, 10, 12),
                FONT_SMALL,
                ink.text_3,
            ));
        }
        children.push(tagged_label(
            &format!("pv.tree.field.{path}"),
            name,
            Rect::new(indent + 6, row.y + 5, 128, 12),
            FONT_SMALL,
            if layer.is_some() {
                ink.text
            } else {
                ink.text_2
            },
        ));
        // The badge is placed from the right edge first and the value takes
        // what is left — the same rule the message rows use, for the same
        // reason the sweep found there.
        let mut right = rect.w.saturating_sub(PAD);
        // The derived arm, shown rather than folded into "no bytes": a reader
        // must be able to tell a computed value from one nobody mapped.
        if map.extent_of(&path).is_none() && map.field(&path).is_some() {
            let width = run_box("derived", 0, 0).w;
            right = right.saturating_sub(width);
            children.push(tagged_label(
                &format!("pv.tree.derived.{path}"),
                "derived",
                Rect::new(right, row.y + 5, width, 12),
                FONT_SMALL,
                ink.text_3,
            ));
        }
        children.push(label(
            value,
            Rect::new(
                indent + 140,
                row.y + 5,
                right.saturating_sub(indent + 148),
                12,
            ),
            FONT_SMALL,
            ink.text,
        ));
    }
    panel(
        "pv.tree",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![scroll_pane(
            &state.tree_scroll,
            panel_content(rect),
            (0, PAD),
            PanePointer::PassesThrough,
            children,
        )],
    )
}

fn bytes_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = bytes_rect();
    let buffer = state.frame_bytes();
    let layout = hex_layout();
    let lit = state.lit_selection();
    let map = state.map.map();
    let marks = map.marks(SourceId::new(0));
    let selected = state.field.get();
    let mut children = vec![
        tagged_label(
            "pv.bytes.title",
            spec::PANES[2].title,
            Rect::new(PAD, 6, 80, 12),
            FONT_SMALL,
            ink.text_3,
        ),
        tagged_label(
            "pv.bytes.span",
            lit.map_or_else(
                || format!("{selected} · no bytes here"),
                |sel| {
                    format!(
                        "{selected} · 0x{:02x}..0x{:02x}",
                        sel.start(),
                        sel.end() - 1
                    )
                },
            ),
            Rect::new(PAD + 84, 6, rect.w.saturating_sub(PAD * 2 + 84), 12),
            FONT_SMALL,
            ink.accent,
        ),
    ];
    for row in 0..layout.rows() {
        let y = HEAD_H + u32::try_from(row).unwrap_or(0) * CELL_H;
        children.push(label(
            format!("{:04x}", row * spec::BYTES_PER_ROW),
            Rect::new(PAD, y + 3, 34, 12),
            FONT_MONO,
            ink.text_3,
        ));
    }
    for (byte, value) in buffer.iter().enumerate() {
        let Some(cell) = byte_cell(byte) else {
            continue;
        };
        let inside = lit.is_some_and(|sel| sel.contains(byte));
        if inside {
            children.push(box_at(
                &format!("pv.bytes.lit.{byte}"),
                Rect::new(cell.x - 1, cell.y + 1, cell.w + 2, cell.h - 2),
                ink.lit,
                Some(ink.accent),
                2,
            ));
        }
        children.push(tagged_label(
            &format!("pv.bytes.cell.{byte}"),
            format!("{value:02x}"),
            Rect::new(cell.x, cell.y + 3, cell.w, 12),
            FONT_MONO,
            if inside {
                ink.accent
            } else if marks.top_at(byte).is_some() {
                ink.text
            } else {
                ink.text_3
            },
        ));
    }
    panel(
        "pv.bytes",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![scroll_pane(
            &state.bytes_scroll,
            panel_content(rect),
            (0, PAD),
            PanePointer::PassesThrough,
            children,
        )],
    )
}

fn reassembly_strip(ink: Ink) -> Scene {
    let rect = reassembly_rect();
    let (done, running, dropped) = spec::REASSEMBLY;
    let mut children = vec![
        tagged_label(
            "pv.reassembly.title",
            "reassembly · sequence continuity per channel",
            Rect::new(PAD, 10, 300, 12),
            FONT_SMALL,
            ink.text_3,
        ),
        tagged_label(
            "pv.reassembly.counts",
            format!(
                "{} of {} channels carrying · done {} · in progress {} · abandoned {}",
                spec::LANES.len(),
                spec::CHANNELS,
                comma(done),
                running,
                dropped
            ),
            Rect::new(rect.w.saturating_sub(430), 10, 414, 12),
            FONT_SMALL,
            ink.text_2,
        ),
    ];
    for (n, lane) in spec::LANES.iter().enumerate() {
        let seat = lane_rect(n);
        let local = Rect::new(seat.x, seat.y - rect.y, seat.w, seat.h);
        children.push(box_at(
            &format!("pv.reassembly.lane.{n}"),
            local,
            ink.surface,
            Some(if lane.continuous {
                ink.outline
            } else {
                ink.err
            }),
            4,
        ));
        children.push(label(
            lane.name,
            Rect::new(local.x + 10, local.y + 8, local.w - 20, 12),
            FONT_SMALL,
            ink.text,
        ));
        children.push(label(
            if lane.continuous {
                format!("{} · unbroken", lane.sn)
            } else {
                format!("{} · {} abandoned", lane.sn, lane.dropped)
            },
            Rect::new(local.x + 10, local.y + 24, local.w - 20, 12),
            FONT_SMALL,
            if lane.continuous { ink.text_2 } else { ink.err },
        ));
    }
    panel("pv.reassembly", rect, ink.bg, Some(ink.outline), children)
}

// ── The External ────────────────────────────────────────────────────────────

/// The screen's own oracle: the one `External` every press is delivered to, and
/// the surface the wire drives the screen through.
struct ViewOracle {
    state: Option<Rc<ViewState>>,
    surface: (u32, u32),
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
        Self {
            state: None,
            surface: (WIN_W, WIN_H),
        }
    }

    fn attach(&mut self, state: Rc<ViewState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<ViewState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected("the screen is not attached yet"))
    }

    fn text(args: &IntrospectValue) -> Result<String, InvokeError> {
        args.as_str()
            .map(str::to_owned)
            .ok_or_else(|| InvokeError::rejected("expected a string argument"))
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
        // ★ R1656 — the fraction is of the LIVE surface, so the basis is the
        // size the shell reported rather than the design constants.
        let (w, h) = self.surface;
        let px = (x_rel.clamp(0.0, 1.0) * w as f32) as u32;
        let py = (y_rel.clamp(0.0, 1.0) * h as f32) as u32;
        move_cursor(&state, px, py);
    }

    /// §5.15 — the shell's resize notification, which is what a pointer
    /// fraction is a fraction of.
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

impl ExternalIntrospect for ViewOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("spec", "json"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("selected_row", "int"),
                    SchemaField::new("selected_field", "string"),
                    SchemaField::new("selected_span", "json"),
                    SchemaField::new("visible_fields", "json"),
                    SchemaField::new("saved", "json"),
                    SchemaField::new("folded", "json"),
                    SchemaField::new("said", "string"),
                    SchemaField::new("cursor", "json"),
                    SchemaField::parametric(
                        "hit.<x>.<y>",
                        "string",
                        const { &[SchemaArg::open("x", "int"), SchemaArg::open("y", "int")] },
                    ),
                    SchemaField::action("select_message", "int"),
                    SchemaField::action("select_field", "string"),
                    SchemaField::action("select_byte", "int"),
                    SchemaField::action("toggle_saved", "int"),
                    SchemaField::action("toggle_layer", "int"),
                    SchemaField::action("point", "string"),
                    SchemaField::action("press", "string"),
                    // ★ R1664 — declared, not merely handled. The router's press
                    // verb was the one action this screen could be driven by and
                    // the one it did not publish, so `rpc/schema` described a
                    // widget that answered a verb it never mentioned and did not
                    // answer the verb it is actually pressed with.
                    SchemaField::action("send", "string"),
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
            return Ok(IntrospectValue::Text(match Hit::at(state, px, py) {
                Hit::Message(n) => format!("message.{n}"),
                Hit::Field(p) => format!("field.{p}"),
                Hit::Byte(b) => format!("byte.{b}"),
                Hit::Saved(n) => format!("saved.{n}"),
                Hit::Layer(n) => format!("layer.{n}"),
                Hit::None => "none".to_owned(),
            }));
        }
        match path {
            "spec" => Ok(IntrospectValue::Json(spec_json())),
            "row_count" => Ok(IntrospectValue::Int(
                i64::try_from(spec::ROWS.len()).unwrap_or(i64::MAX),
            )),
            "selected_row" => Ok(IntrospectValue::Int(
                i64::try_from(state.row.get()).unwrap_or(i64::MAX),
            )),
            "selected_field" => Ok(IntrospectValue::Text(state.field.get())),
            "selected_span" => Ok(state.lit_selection().map_or(IntrospectValue::Null, |sel| {
                IntrospectValue::Json(serde_json::json!({
                    "start": sel.start(),
                    "end": sel.end(),
                }))
            })),
            "visible_fields" => Ok(IntrospectValue::Json(serde_json::Value::Array(
                visible_fields(state)
                    .into_iter()
                    .map(|(p, ..)| serde_json::Value::String(p))
                    .collect(),
            ))),
            "saved" => Ok(IntrospectValue::Json(serde_json::json!(state.saved.get()))),
            "folded" => Ok(IntrospectValue::Json(serde_json::json!(state.folded.get()))),
            "said" => Ok(IntrospectValue::Text(state.said.borrow().clone())),
            "cursor" => {
                let (x, y) = state.cursor.get();
                Ok(IntrospectValue::Json(serde_json::json!({"x": x, "y": y})))
            }
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        if self.query(path).is_ok() {
            Err(InterveneError::ReadOnly)
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        match path {
            "select_message" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a row index"))?;
                if n >= spec::ROWS.len() {
                    return Err(InvokeError::rejected(format!(
                        "no message {n} was captured"
                    )));
                }
                select_message(&state, n);
                Ok(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)))
            }
            "select_field" => {
                let want = Self::text(&args)?;
                let want = want.trim();
                if state.map.map().field(want).is_none() {
                    return Err(InvokeError::rejected(format!(
                        "this decode has no field at {want:?}"
                    )));
                }
                select_field(&state, want);
                Ok(IntrospectValue::Text(want.to_owned()))
            }
            "select_byte" => {
                let b = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a byte offset"))?;
                select_byte(&state, b);
                Ok(IntrospectValue::Text(state.field.get()))
            }
            "toggle_saved" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a filter index"))?;
                if n >= spec::SAVED_FILTERS.len() {
                    return Err(InvokeError::rejected(format!("no saved filter {n}")));
                }
                toggle_saved(&state, n);
                Ok(IntrospectValue::Json(serde_json::json!(state.saved.get())))
            }
            "toggle_layer" => {
                let n = args
                    .as_usize()
                    .ok_or_else(|| InvokeError::rejected("expected a layer index"))?;
                if n >= spec::LAYERS.len() {
                    return Err(InvokeError::rejected(format!("no layer {n}")));
                }
                toggle_layer(&state, n);
                Ok(IntrospectValue::Json(serde_json::json!(state.folded.get())))
            }
            "point" => {
                let raw = Self::text(&args)?;
                let (x, y) = raw
                    .trim()
                    .split_once(',')
                    .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <x>,<y>")))?;
                let px = x
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{x:?} is not a coordinate")))?;
                let py = y
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{y:?} is not a coordinate")))?;
                move_cursor(&state, px, py);
                Ok(IntrospectValue::Text(format!("{px},{py}")))
            }
            "press" => {
                press(&state);
                Ok(IntrospectValue::Text(state.said.borrow().clone()))
            }
            // ★★★★ R1664 — `send` is the verb the §5.35 ROUTER presses with,
            // and it is the second half of this screen's deafness.
            //
            // Routing a press is two joins, and R1663 got neither: the painted
            // root has to carry the tag the widget is registered under (the
            // repair in `view` above), and the widget has to answer the verb
            // `dispatch_send` dispatches — `invoke("send", "PointerDown")`. This
            // screen spelled its own action `press`, so once the first join was
            // repaired the router resolved the widget, called `send`, got
            // `UnknownPath` back, and **discarded it** (`let _ = intro.invoke(…)`
            // in `pinion_runtime::input::dispatch_send`): a press that arrives,
            // is refused, and leaves no trace anywhere.
            //
            // Measured before and after this arm existed, driving `scene/click
            // {at}` — the wire entry point that goes through the router — rather
            // than `invoke("press")`, which is the oracle and was never broken.
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => press(&state),
                    // A release commits nothing on this screen: selection is
                    // decided at press, and there is no drag. Accepted rather
                    // than rejected, because the router sends the pair and a
                    // widget that refuses half of it is the shape above.
                    "PointerUp" | "PointerEnter" | "PointerLeave" | "PointerCancel" => {}
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerEnter / PointerLeave / PointerCancel"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.said.borrow().clone()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(key(&state, chord.trim())))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The whole specification, as the wire sees it — so the demo reads the table
/// from the running application rather than keeping a second copy of it.
fn spec_json() -> serde_json::Value {
    serde_json::json!({
        "panes": spec::PANES.iter().map(|p| serde_json::json!({
            "tag": p.tag, "title": p.title, "width": p.width, "body": p.body,
        })).collect::<Vec<_>>(),
        "columns": spec::COLUMNS.iter().map(|c| serde_json::json!({
            "title": c.title, "width": c.width,
        })).collect::<Vec<_>>(),
        "kinds": spec::KINDS,
        "context": spec::CONTEXT.iter().map(|c| serde_json::json!({
            "key": c.key, "value": c.value, "note": c.note,
        })).collect::<Vec<_>>(),
        "saved_filters": spec::SAVED_FILTERS,
        "query_clauses": spec::QUERY_CLAUSES,
        "layers": spec::LAYERS.iter().map(|(id, title)| serde_json::json!({
            "id": id, "title": title,
        })).collect::<Vec<_>>(),
        "fields": spec::FIELDS.iter().map(|f| serde_json::json!({
            "path": f.path, "name": f.name, "value": f.value,
            "source": f.source, "at": f.at, "len": f.len,
        })).collect::<Vec<_>>(),
        "rows": spec::ROWS.iter().map(|r| serde_json::json!({
            "time": r.time, "hop": r.hop, "channel": r.channel, "sn": r.sn,
            "kind": r.kind, "name": r.name, "len": r.len, "note": r.note,
            "fragment": r.fragment.as_ref().map(|f| serde_json::json!({
                "marker": f.marker, "piece": f.piece,
            })),
        })).collect::<Vec<_>>(),
        "lanes": spec::LANES.iter().map(|l| serde_json::json!({
            "name": l.name, "sn": l.sn, "continuous": l.continuous, "dropped": l.dropped,
        })).collect::<Vec<_>>(),
        "sources": spec::SOURCES.iter().map(|(n, l)| serde_json::json!({
            "name": n, "len": l,
        })).collect::<Vec<_>>(),
        "matched": spec::MATCHED,
        "captured": spec::CAPTURED,
        "opening_row": spec::OPENING_ROW,
        "opening_field": spec::OPENING_FIELD,
    })
}

// ── The binding ─────────────────────────────────────────────────────────────

struct PacketView;

impl WidgetCore for PacketView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ViewOracle::new();
        oracle.attach(use_view_state());
        Box::new(oracle)
    }

    /// The byte map is published as its own External, because the relation is
    /// the crate's and a consumer that re-published it here would be the second
    /// copy this round removed.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            MAP_TAG,
            Box::new(ByteMapExternal::new(Rc::clone(&use_view_state().map))),
        )]
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
        "pinion hello-packet-view (R1663 §5.41 field-to-bytes capture viewer)"
    }

    fn apply_key(
        scene: &mut Scene,
        _focused: Option<&str>,
        chord: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Some(node) = scene.find_external_with_tag_mut(VIEW_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke("key", IntrospectValue::Text(chord.to_owned()))
            .is_ok_and(|v| v.as_bool() == Some(true))
    }
}

impl WidgetA11y for PacketView {
    /// The screen as three panes with a value each, so an assistive client
    /// hears which message is open, which field is selected and what it covers
    /// — the same three facts the wire answers.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_view_state();
        let map = state.map.map();
        let field = state.field.get();
        let span = map.extent_of(&field).map_or_else(
            || format!("{field}, derived, no bytes"),
            |(source, extent)| {
                format!(
                    "{field}, {} bytes at offset {} of {}",
                    extent.len(),
                    extent.at(),
                    map.sources()[source.index()].name()
                )
            },
        );
        vec![
            AccessNode::new("pv.list", AriaRole::Table)
                .with_name("Messages")
                .with_value(AccessValue::Text(format!(
                    "message {} of {}",
                    state.row.get() + 1,
                    spec::ROWS.len()
                ))),
            AccessNode::new("pv.tree", AriaRole::Tree)
                .with_name("Decode")
                .with_value(AccessValue::Text(span.clone())),
            AccessNode::new("pv.bytes", AriaRole::Group)
                .with_name("Bytes")
                .with_value(AccessValue::Text(span)),
        ]
    }
}

impl WidgetView for PacketView {
    type Renderer = HelloPacketViewRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PacketView>();
}
