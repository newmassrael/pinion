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

use pinion_a11y::{
    AccessFocus, AccessLive, AccessNode, AccessState, AccessValue, AriaRole, GridCell, GridColumn,
    GridRow, WidgetA11y, grid_table_nodes,
};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::focus_state;
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextOverflow, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::voice::Silence;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::field_bytes::{
    ByteExtent, ByteMap, ByteMapExternal, ByteMapState, ByteSource, Coverage, FieldSpan, SourceId,
    use_byte_map,
};
use pinion_core::widgets::hex_dump::{ByteSelection, HexLayout};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::roving::{Activation, Axis, Landing, Member, Roving, RovingSpec};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;

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

/// How many characters `text` has, at compile time.
///
/// UTF-8 continuation bytes are the ones matching `0b10xxxxxx`; every other byte
/// starts a character. Written out because `str::chars` is not `const`, and the
/// floor below has to be **derived** from the strings rather than measured once
/// by hand — which is the whole lesson [`NAME_FLOOR`] records.
const fn char_count(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] & 0b1100_0000 != 0b1000_0000 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// The width a small run of `text` occupies. The `const` half of [`run_box`],
/// so a floor derived from it and the box actually painted cannot disagree.
const fn run_width(text: &str) -> u32 {
    char_count(text) * (FONT_SMALL - 4) + 10
}

/// The width one message's annotations take out of the name column: the note,
/// and the fragment marker with the gap that separates it.
const fn annotations_width(row: &spec::RowSpec) -> u32 {
    let mut width = 0;
    if !row.note.is_empty() {
        width += run_width(row.note);
    }
    if let Some(fragment) = &row.fragment {
        // The marker is painted as `{marker} {piece}`, and the 8 is the gap the
        // painter subtracts before placing it.
        width += char_count(fragment.marker) * (FONT_SMALL - 4)
            + char_count(fragment.piece) * (FONT_SMALL - 4)
            + (FONT_SMALL - 4)
            + 10
            + 8;
    }
    width
}

/// The widest annotation load any message in the capture puts on the name
/// column.
const fn widest_annotations() -> u32 {
    let mut widest = 0;
    let mut i = 0;
    while i < spec::ROWS.len() {
        let width = annotations_width(&spec::ROWS[i]);
        if width > widest {
            widest = width;
        }
        i += 1;
    }
    widest
}

/// The narrowest the resource name itself may be painted — about seven
/// characters and an ellipsis, which is what the design size gives it today.
const NAME_RUN_FLOOR: u32 = 60;

/// The narrowest the flexible `name` column may be and still show a resource
/// path rather than an ellipsis alone.
///
/// ★★★★★ R1693 — this was `180`, a number somebody picked, and the per-cell
/// tagging the accessibility round added is what finally failed on it: at the
/// declared floor the annotations of the reassembled message
/// (`Last 3/3` + `reassembled 3,144 B`) are wider than the whole column, so the
/// resource name was painted at **zero width** and simply was not there. A
/// sighted reader saw a message with no name; nothing failed, because nothing
/// had ever asked whether that cell was painted.
///
/// It is the same defect the comment on [`LIST_FLOOR`] already recorded one
/// level up — a floor somebody picks can be wrong about the thing it is a floor
/// FOR — and the same repair: derive it from what the column has to hold.
const NAME_FLOOR: u32 = widest_annotations() + NAME_RUN_FLOOR;

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
    /// ★★★★★ R1698 — which byte the hex grid's keyboard cursor rests on.
    ///
    /// **Not** derivable from [`field`](Self::field), and the round found that
    /// out by trying: what the grid LIGHTS is the selected field's extent, and
    /// a cursor computed from it cannot move within a field at all — every
    /// arrow snapped back to the field's first byte. Selection and cursor are
    /// two facts in every grid that has both (WAI-ARIA gives them
    /// `aria-selected` and `aria-activedescendant` for the same reason), and
    /// collapsing them is what made the byte grid's arrows a no-op.
    ///
    /// A press writes it too, so the pointer and the keyboard never disagree
    /// about where the cursor is.
    byte: Signal<usize>,
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
        // The cursor opens on the first byte of the field the screen opens
        // with, so the two agree at boot and diverge only once somebody moves
        // one of them. Derived rather than written down, because a second
        // literal would be a second thing to keep in step with the field.
        byte: Signal::new(
            map.map()
                .extent_of(spec::OPENING_FIELD)
                .map_or(0, |(_, extent)| extent.at()),
        ),
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

/// A run that can be addressed — the lifted
/// [`text_run`], which is where the
/// rectangle-used-twice and pointer-transparency decisions now live (R1694,
/// the third identical copy).
fn tagged_label(tag: &str, text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    text_run(tag, text, rect, run_style(px, fg))
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

// ★★★★★ R1693 — this screen had NO keyboard stop. It announced three `button`
// chips and three composite panes and a keyboard user could reach none of them,
// which is the same defect as announcing a `table` with no rows, one axis over:
// a role that promises something the screen cannot do.
//
// The ring is the WAI-ARIA composite pattern: **one stop per composite** (the
// grid, the tree, the byte grid — arrows move *within* them, which is what this
// screen's `ArrowUp`/`ArrowDown` already do) plus one per plain button.
//
// ★ R1696 — the four-line helper that declared it is gone: the sibling screen
// needed the same thing and a verbatim copy is how a mechanism becomes two, so
// it is `Scene::with_focusable` now. Same lift, same evidence and the same
// second-consumer trigger as `Scene::silenced` at R1693.

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
    // ★ R1698 — the cursor goes where the byte is, whether a press or an arrow
    // brought us here, so the pointer and the keyboard never disagree about
    // where the grid's cursor rests.
    state.byte.set(byte);
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

/// ★★★★★ R1698 — **the cursor each pane already has, said in the framework's
/// vocabulary.**
///
/// Not new state: the message list's cursor IS `row`, the decode tree's IS
/// `field`, and the byte grid's is the byte the map has selected. A second copy
/// of any of them would be a second thing to keep in step. So this projects
/// what the screen holds into a [`Roving`] — which is what makes the arrows
/// scopable, the active descendant publishable, and the policy askable, without
/// the screen owning a cursor twice.
///
/// All three declare [`Activation::Follows`], and that is the substantive
/// difference from the sibling screen: here the cursor **is** the selection —
/// moving down a message list means reading the next message — while a
/// navigation rail whose selection followed its cursor would navigate away from
/// the page a reader is trying to leave.
fn pane_cursor(state: &Rc<ViewState>, stop: &str) -> Option<Roving> {
    let (spec, members, at) = match stop {
        "pv.list" => (
            RovingSpec::new(Axis::Vertical).with_activation(Activation::Follows),
            (0..spec::ROWS.len())
                .map(|n| Member::new(format!("pv.list.row.{n}")))
                .collect::<Vec<_>>(),
            format!("pv.list.row.{}", state.row.get()),
        ),
        "pv.tree" => (
            RovingSpec::new(Axis::Vertical).with_activation(Activation::Follows),
            visible_fields(state)
                .iter()
                .map(|(path, ..)| Member::new(format!("pv.tree.field.{path}")))
                .collect(),
            format!("pv.tree.field.{}", state.field.get()),
        ),
        "pv.bytes" => (
            // Both axes, because the grid wraps: a byte's neighbour to the
            // right is the next byte and the one below is sixteen further on,
            // and both are steps along the SAME linear buffer. `Both` is the
            // arm ARIA leaves undefined rather than calling horizontal.
            RovingSpec::new(Axis::Both).with_activation(Activation::Follows),
            (0..state.frame_bytes().len())
                .map(|b| Member::new(format!("pv.bytes.cell.{b}")))
                .collect(),
            format!("pv.bytes.cell.{}", state.byte.get()),
        ),
        _ => return None,
    };
    let mut roving = Roving::new(spec);
    roving.seat(members);
    roving.point_at(&at);
    Some(roving)
}

/// R1698 — put a pane's cursor where a [`Roving`] left it.
///
/// The write half of [`pane_cursor`]'s projection: one place, so a cursor that
/// moved and a selection that did not is not a state this screen can be in.
fn seat_pane_cursor(state: &Rc<ViewState>, stop: &str, roving: &Roving) {
    let Some(index) = roving.cursor() else { return };
    match stop {
        "pv.list" => select_message(state, index),
        "pv.tree" => {
            if let Some((path, ..)) = visible_fields(state).get(index) {
                select_field(state, &path.clone());
            }
        }
        "pv.bytes" => select_byte(state, index),
        _ => {}
    }
}

fn key(state: &Rc<ViewState>, chord: &str) -> bool {
    key_at(state, focus_state::focused().as_deref(), chord)
}

/// ★★★★★ R1698 — **the keymap, told where the reader is standing.**
///
/// Measured before this existed, by driving the running application: at all
/// SIX of this screen's Tab stops — three filter chips, the decode tree and the
/// byte grid included — pressing `ArrowDown` moved the **message list**, and
/// the active descendant was `None` everywhere. An arrow meant one thing no
/// matter where anybody was standing, which is the other half of the composite
/// pattern R1693 left open when it gave the panes their Tab stops.
///
/// A plain button is its own stop and owns no cursor, so a chord arriving there
/// falls through to the screen — which is why `Escape` still works from
/// anywhere and why the pane arrows no longer do.
fn key_at(state: &Rc<ViewState>, focused: Option<&str>, chord: &str) -> bool {
    if let Some(stop) = focused
        && let Some(mut roving) = pane_cursor(state, stop)
        && let Some(landing) = roving.key(chord)
    {
        if let Landing::Moved { choose: true, .. } = landing {
            seat_pane_cursor(state, stop, &roving);
        }
        return true;
    }
    match chord {
        // ★★★★★ R1693 — the chords a real keyboard sends. They were `Down` and
        // `Up`, which **no key press produces**: the shell spells a named key
        // the way the web platform does (`ArrowDown`), and this screen is the
        // only place in the tree that spelled them short. Measured through the
        // wire, `ArrowDown` moved nothing and `Down` moved the selection — so
        // this screen's keyboard navigation had never worked from a keyboard,
        // and every test that drove it passed because every test used the
        // screen's own spelling.
        //
        // The round that found it is the round that announced this list as a
        // `grid`, which is a composite widget whose contract IS that the arrows
        // move the selection. Announcing that while the arrows are dead is the
        // same class of defect as announcing a table with no rows.
        // ★★★★★ R1698 — **the message list's arrows belong to the message
        // list.** A chord a composite does not navigate by falls through, and
        // this is where it used to land: measured on the running screen,
        // standing on any of the three saved-filter chips and pressing Down
        // moved a row in a pane the reader was not in.
        //
        // `None` still reaches it, deliberately — that is the wire's own
        // channel (`invoke("key", …)` with nothing focused), where an agent
        // asking the list to move its selection is asking for exactly that.
        "ArrowDown" | "ArrowUp" if focused.is_none_or(|tag| tag == "pv.list") => {
            let row = state.row.get();
            let next = if chord == "ArrowDown" {
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
        ContainerNode::new(vec![
            panel(
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
            )
            .silenced(Silence::layout(
                "places the two bars, the three panes and the reassembly strip",
            )),
        ])
        // ★ R1664 — the root carries the tag the widget is REGISTERED under, so
        // the router has something to resolve a press to. `pv.root` above is an
        // ADDRESS, for `scene/snapshot` and the sweep; this is the RECEIVER.
        // They were two string literals in two functions with nothing checking
        // that either of them named anything, and the screen was dead at every
        // point in the window. `scene/pointer_reach`.externals is the read that
        // now holds both sides of that join.
        .with_tag(VIEW_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(w, h))
                // The receiver is an address for presses, not a region a reader
                // travels to: everything it holds is `pv.root`, which says so
                // for itself.
                .with_silence(Silence::layout(
                    "the window's receiver; it holds the screen",
                )),
        ),
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
        children.push(
            box_at(
                &format!("pv.filter.saved.{n}"),
                Rect::new(chip.x, chip.y - rect.y, chip.w, chip.h),
                if on { ink.lit } else { ink.surface },
                Some(if on { ink.accent } else { ink.outline }),
                11,
            )
            // A plain button is its own stop.
            .with_focusable(true),
        );
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
    Rect::new(x, y, run_width(text), 14)
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
    // ★ One stop for the whole grid — the WAI-ARIA composite pattern, and the
    // one this screen already behaves like: the arrows move the selection
    // *inside* it rather than off it.
    panel(
        "pv.list",
        rect,
        ink.bg,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.list_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the message grid; the rows inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

/// One message row, in the list pane's own coordinates.
fn list_row_paint(n: usize, selected: usize, ink: Ink) -> Vec<Scene> {
    let message = &spec::ROWS[n];
    let mut children = Vec::new();
    {
        let row = list_row(n);
        if n == selected {
            children.push(
                box_at("pv.list.selected", row, ink.lit, Some(ink.accent), 0).silenced(
                    Silence::decorative(
                        "the band behind the open message; the row says it is selected",
                    ),
                ),
            );
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
        // ★★★★★ R1693 — every cell carries its own tag, which is what makes the
        // list a grid a reader can traverse rather than sixteen paragraphs. The
        // floor's item view answers `cellAt(row, col)` from its model, so a
        // hand-painted table that only tagged its rows would be strictly less
        // navigable than the thing it is meant to beat — measured at 6.11.1,
        // `cellAt(3, 4)` names the cell and reports its row, column and column
        // header, while a custom-painted pane answers one node with no children
        // at all. Tagged on the TEXT rather than in a new box: a container per
        // cell would sit above the row in paint order and take the press the row
        // needs (`scene/pointer_reach` refused exactly that shape in R1692).
        let text = ink.text;
        let texts = cell_texts(message);
        for (c, ink_for) in [
            ink.text_2,
            text,
            ink.text_2,
            ink.text_2,
            kind_ink(message.kind),
        ]
        .into_iter()
        .enumerate()
        {
            children.push(cell_label(n, c, texts[c].clone(), cell(c), ink_for));
        }
        // ★ The name column is shared with the row's annotations, so the
        // annotations are placed FIRST, from the right edge inward, and the
        // name takes what is left. The first draft gave the name the whole
        // column and put the annotations at fixed offsets inside it; the
        // sweep's first run found `piece 1 of 3` painted underneath `First
        // 1/3`. Subtracting what is already placed cannot produce that,
        // whatever the strings turn out to be.
        let name_col = cell(NAME_COLUMN);
        let mut right = name_col.x + name_col.w;
        if !message.note.is_empty() {
            let width = run_box(message.note, 0, 0).w;
            right = right.saturating_sub(width);
            children.push(
                tagged_label(
                    &format!("pv.list.row.{n}.note"),
                    message.note,
                    Rect::new(right, row.y + 5, width, 12),
                    FONT_SMALL,
                    ink.warn,
                )
                // Painted inside the name column and announced as part of that
                // cell: an annotation read as its own stop would tell a reader
                // "out of band" with nothing to attach it to.
                .silenced(Silence::part_of(list_cell_tag(n, NAME_COLUMN))),
            );
        }
        if let Some(fragment) = &message.fragment {
            let marker = format!("{} {}", fragment.marker, fragment.piece);
            let width = run_box(&marker, 0, 0).w;
            right = right.saturating_sub(width + 8);
            children.push(
                tagged_label(
                    &format!("pv.list.row.{n}.fragment"),
                    marker,
                    Rect::new(right, row.y + 5, width, 12),
                    FONT_SMALL,
                    if fragment.marker == "Drop" {
                        ink.err
                    } else {
                        ink.warn
                    },
                )
                .silenced(Silence::part_of(list_cell_tag(n, NAME_COLUMN))),
            );
        }
        children.push(cell_label(
            n,
            NAME_COLUMN,
            texts[NAME_COLUMN].clone(),
            Rect::new(
                name_col.x,
                row.y + 5,
                right.saturating_sub(name_col.x + 8),
                12,
            ),
            text,
        ));
        children.push(cell_label(n, 6, texts[6].clone(), cell(6), ink.text_2));
    }
    children
}

/// The tag one message cell is addressed by: the row and the column it is in.
///
/// A function rather than a `format!` at each of the seven sites, because the
/// spelling is a join — [`spec::ROWS`] crossed with [`spec::COLUMNS`] — and both
/// the paint and [`spec::VOICES`] have to produce it from the same rule or the
/// census would be comparing two conventions.
#[must_use]
pub fn list_cell_tag(row: usize, column: usize) -> String {
    format!("pv.list.cell.{row}_{column}")
}

/// One message cell, tagged so a reader can traverse the grid a column at a
/// time. Its accessible name is the text painted here, which is why nothing
/// re-states the value in the accessibility layer.
fn cell_label(row: usize, column: usize, text: impl Into<String>, rect: Rect, fg: Color) -> Scene {
    tagged_label(&list_cell_tag(row, column), text, rect, FONT_SMALL, fg)
}

/// One decode row: its selection band, its fold chevron, its name, its derived
/// badge and its value.
///
/// Split out of [`tree_pane`] at R1693, when the per-run silence declarations
/// took that function past the hundred-line bound. The seam is the one the
/// message list already uses (`list_row_paint`), so the two panes read alike.
fn tree_row_paint(
    state: &Rc<ViewState>,
    n: usize,
    (path, name, value, depth): &(String, String, String, usize),
    selected: &str,
    ink: Ink,
) -> Vec<Scene> {
    let rect = tree_rect();
    let map = state.map.map();
    let folded = state.folded.get();
    let row = tree_row(n);
    let layer = spec::LAYERS.iter().position(|(id, _)| *id == path.as_str());
    let mut children = Vec::new();
    if path == selected {
        children.push(
            box_at("pv.tree.selected", row, ink.lit, Some(ink.accent), 0).silenced(
                Silence::decorative("the band behind the open field; the item says it is selected"),
            ),
        );
    }
    let indent = PAD + u32::try_from(*depth).unwrap_or(0) * 14;
    if let Some(index) = layer {
        children.push(
            tagged_label(
                &format!("pv.tree.layer.{}", spec::LAYERS[index].0),
                if folded.get(index).copied().unwrap_or(false) {
                    ">"
                } else {
                    "v"
                },
                Rect::new(PAD - 6, row.y + 5, 10, 12),
                FONT_SMALL,
                ink.text_3,
            )
            // ★ The fold chevron. `v` and `>` are a picture of a state ARIA has
            // a word for, and the item carries it as `aria-expanded` —
            // announcing the glyph too would read a punctuation mark aloud
            // beside the thing it already said.
            .silenced(Silence::part_of(format!("pv.tree.field.{path}"))),
        );
    }
    children.push(tagged_label(
        &format!("pv.tree.field.{path}"),
        name.clone(),
        Rect::new(indent + 6, row.y + 5, 128, 12),
        FONT_SMALL,
        if layer.is_some() {
            ink.text
        } else {
            ink.text_2
        },
    ));
    // The badge is placed from the right edge first and the value takes what is
    // left — the same rule the message rows use, for the same reason the sweep
    // found there.
    let mut right = rect.w.saturating_sub(PAD);
    // The derived arm, shown rather than folded into "no bytes": a reader must
    // be able to tell a computed value from one nobody mapped.
    if map.extent_of(path).is_none() && map.field(path).is_some() {
        let width = run_box("derived", 0, 0).w;
        right = right.saturating_sub(width);
        children.push(
            tagged_label(
                &format!("pv.tree.derived.{path}"),
                "derived",
                Rect::new(right, row.y + 5, width, 12),
                FONT_SMALL,
                ink.text_3,
            )
            // The badge says this value came from no bytes. That is a fact about
            // the field, announced with it rather than as a separate stop that
            // says only "derived".
            .silenced(Silence::part_of(format!("pv.tree.field.{path}"))),
        );
    }
    children.push(label(
        value.clone(),
        Rect::new(
            indent + 140,
            row.y + 5,
            right.saturating_sub(indent + 148),
            12,
        ),
        FONT_SMALL,
        ink.text,
    ));
    children
}

fn tree_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let rect = tree_rect();
    let selected = state.field.get();
    let mut children = vec![
        tagged_label(
            "pv.tree.title",
            format!("{}  ·  L0 -> L3", spec::PANES[1].title),
            Rect::new(PAD, 6, 200, 12),
            FONT_SMALL,
            ink.text_3,
        )
        .silenced(Silence::name_of("pv.tree")),
    ];
    for (n, field) in visible_fields(state).into_iter().enumerate() {
        children.extend(tree_row_paint(state, n, &field, selected.as_str(), ink));
    }
    // One stop for the tree, like the grid beside it.
    panel(
        "pv.tree",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.tree_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the decode tree; the items inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
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
        )
        .silenced(Silence::name_of("pv.bytes")),
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
        // ★★★ R1693 — the offset is this row's HEADER, tagged so it is one.
        // Measured at 6.11.1, a cell in an item view answers `rowHeaderCells`,
        // so a byte pane whose rows had no header would be less locatable than
        // the floor — while the same pane painted by hand there answers a single
        // node with no children at all.
        children.push(tagged_label(
            &bytes_offset_tag(row),
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
            children.push(
                box_at(
                    &format!("pv.bytes.lit.{byte}"),
                    Rect::new(cell.x - 1, cell.y + 1, cell.w + 2, cell.h - 2),
                    ink.lit,
                    Some(ink.accent),
                    2,
                )
                .silenced(Silence::decorative(
                    "the highlight behind a byte the open field was read from; the \
                     pane's readout says which bytes those are",
                )),
            );
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
    // And one for the byte grid, so the three composites are three stops.
    panel(
        "pv.bytes",
        rect,
        ink.surface,
        Some(ink.outline),
        vec![
            scroll_pane(
                &state.bytes_scroll,
                panel_content(rect),
                (0, PAD),
                PanePointer::PassesThrough,
                children,
            )
            .silenced(Silence::layout(
                "scrolls the byte grid; the rows inside it are what a reader lands on",
            )),
        ],
    )
    .with_focusable(true)
}

/// The tag one row of the byte grid addresses its offset by.
///
/// Named here for the same reason [`list_cell_tag`] is: the paint and
/// [`spec::VOICES`] both produce this spelling, and a second convention would
/// make the census compare two things.
#[must_use]
pub fn bytes_offset_tag(row: usize) -> String {
    format!("pv.bytes.offset.{row}")
}

/// The tag one row of the byte grid is announced under.
///
/// Nothing paints this: a byte row is the eight cells and the offset beside
/// them, and the row is what a reader descends *through*. It is anchored in the
/// census by the members it composes, which is the exemption the census can
/// check for itself rather than one a screen declares.
#[must_use]
pub fn bytes_row_tag(row: usize) -> String {
    format!("pv.bytes.row.{row}")
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
        )
        .silenced(Silence::name_of("pv.reassembly")),
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
            lane_reading(lane),
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
        // ★★★★★ R1693 — the declared split, **already expanded**. A client
        // reading this gets the tags and the roles rather than a table plus the
        // rule for reading it, so a demo checking what a reader is told does not
        // carry a second copy of the populations — and a family that grows a
        // member grows here, where the demo is looking.
        "voices": spec::VOICES.iter().flat_map(|v| {
            v.population.members().into_iter().map(|member| serde_json::json!({
                "tag": v.tag.replace("{}", &member), "role": v.role,
            }))
        }).collect::<Vec<_>>(),
        "silences": spec::SILENCES.iter().flat_map(|(tag, population, kind)| {
            population.members().into_iter().map(move |member| serde_json::json!({
                "tag": tag.replace("{}", &member), "kind": kind,
            }))
        }).collect::<Vec<_>>(),
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

    /// ★★ R1698 — `focused` is threaded through rather than dropped.
    ///
    /// It was `_focused`, and the census across the tree says that is the
    /// minority position: 135 of 172 `apply_key` implementations read it. What
    /// dropping it cost here was measured on the running screen — at all six
    /// Tab stops, `ArrowDown` moved the message list, including from the decode
    /// tree and the byte grid, which have cursors of their own.
    ///
    /// It goes over the wire as a second argument rather than through a second
    /// channel, so the RPC `invoke("key", …)` path and a real key press reach
    /// the same function with the same information.
    fn apply_key(
        _scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        key_at(&use_view_state(), focused, chord)
    }
}

impl WidgetA11y for PacketView {
    /// ★★★★★ R1698 — where the cursor rests inside the focused pane.
    ///
    /// The panes announce themselves as a `grid`, a `tree` and a `grid`, and
    /// every one of those roles' contract is that the arrows move a cursor an
    /// assistive technology can follow. Measured before this existed: the
    /// active descendant was `None` at every stop, so a reader was told the
    /// list had focus and never which row.
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        let stop = focused?;
        let state = use_view_state();
        let cursor = pane_cursor(&state, stop).and_then(|r| r.cursor_tag().map(str::to_owned));
        Some(AccessFocus::addressing(stop, cursor))
    }

    /// ★★★★★ R1693 — **the screen, announced.**
    ///
    /// It announced three nodes until this round: a `table` with no row, a
    /// `tree` with no item, and a `group`. The 186 painted regions behind them
    /// — sixteen messages of seven columns each, twenty-one decoded fields over
    /// four layers, seventy-two bytes, the filter, the negotiated context and
    /// the reassembly lanes — were not in the accessibility tree at all, and
    /// every check in this example was green because a region with no node
    /// paints perfectly and answers every question about its rectangle.
    ///
    /// The two collection roles are the sharper half: announcing `table` and
    /// holding nothing tells a reader a table is there and gives them nothing to
    /// enter. `scene/conform` is what refuses that now.
    ///
    /// Built pane by pane below, and each pane's population comes from the same
    /// `spec` table the painter reads — so what a reader hears and what is drawn
    /// cannot drift, and `spec::VOICES` can expand the families and check both
    /// directions.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_view_state();
        let mut nodes = app_bar_nodes(&state);
        nodes.extend(filter_nodes(&state));
        nodes.extend(context_nodes());
        nodes.extend(list_nodes(&state));
        nodes.extend(tree_nodes(&state));
        nodes.extend(bytes_nodes(&state));
        nodes.extend(reassembly_nodes());
        // ★★★★★ R1698 — each pane publishes the cursor its arrows move, in one
        // place rather than at three builders. What it publishes is NOT the
        // node's children: the tree's children are its visible fields (which
        // happens to match) while the byte grid's are its ROWS and the cursor
        // walks its cells. A client reading `children` to learn what the arrows
        // reach would be told rows and given cells.
        for node in &mut nodes {
            if let Some(roving) = pane_cursor(&state, &node.tag) {
                *node = node.clone().with_navigation(&roving);
            }
        }
        nodes
    }
}

/// The application bar: what capture is open, how fast it is arriving, and the
/// running commentary.
fn app_bar_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    vec![
        AccessNode::new("pv.appbar", AriaRole::Group)
            .with_name("packet view")
            .with_child("pv.appbar.interface")
            .with_child("pv.appbar.rate")
            .with_child("pv.appbar.said"),
        AccessNode::new("pv.appbar.interface", AriaRole::Status),
        AccessNode::new("pv.appbar.rate", AriaRole::Status),
        // ★ A live region. It opens EMPTY and fills as the screen is driven, so
        // its name is what the region is and the commentary is its value — a
        // name taken from the contents would be absent at boot, which is the
        // `mumbled` defect and not a naming style.
        AccessNode::new("pv.appbar.said", AriaRole::Status)
            .with_name("activity")
            .with_value(AccessValue::Text(state.said.borrow().clone()))
            .with_live(AccessLive::Polite),
    ]
}

/// The filter bar: the query as its clauses, the saved filters as toggles, and
/// how much of the capture matched.
fn filter_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let saved = state.saved.get();
    let mut group = AccessNode::new("pv.filter", AriaRole::Group).with_name("Filter");
    let mut nodes = Vec::new();
    for n in 0..spec::QUERY_CLAUSES.len() {
        let tag = format!("pv.filter.clause.{n}");
        group = group.with_child(tag.clone());
        nodes.push(AccessNode::new(tag, AriaRole::Status));
    }
    for (n, name) in spec::SAVED_FILTERS.iter().enumerate() {
        let tag = format!("pv.filter.saved.{n}");
        group = group.with_child(tag.clone());
        // A toggle button: WAI-ARIA reflects a saved filter's on/off as
        // `aria-pressed`, and the chip paints its label as a SIBLING of its box,
        // so the name comes from the table both readers share.
        nodes.push(
            AccessNode::new(tag, AriaRole::Button)
                .with_name(*name)
                .with_state(AccessState {
                    checked: Some(saved.get(n).copied().unwrap_or(false)),
                    ..AccessState::default()
                }),
        );
    }
    group = group.with_child("pv.filter.count");
    nodes.push(AccessNode::new("pv.filter.count", AriaRole::Status));
    nodes.insert(0, group);
    nodes
}

/// The negotiated session context: six values the decode is only interpretable
/// against, each announced as what it is and what it was negotiated to.
fn context_nodes() -> Vec<AccessNode> {
    let mut group = AccessNode::new("pv.context", AriaRole::Group)
        .with_name("Session context")
        .with_child("pv.context.session");
    let mut nodes = vec![AccessNode::new("pv.context.session", AriaRole::Status)];
    for value in spec::CONTEXT {
        let tag = format!("pv.context.{}", value.key.replace(' ', "_"));
        group = group.with_child(tag.clone());
        // The KEY names the region and the negotiated setting is its value. The
        // painted run carrying the tag holds only the value, so a name derived
        // from the paint would announce `off` and never say what is off.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(value.key)
                .with_value(AccessValue::Text(context_reading(value))),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// What one context value reads as, its consequence note included — the strip
/// paints the two as separate runs and a reader receives them as one fact.
fn context_reading(value: &spec::ContextValue) -> String {
    if value.note.is_empty() {
        value.value.to_owned()
    } else {
        format!("{} · {}", value.value, value.note)
    }
}

/// ★★★★★ R1693 — the message list as a **grid**: a header row of column
/// headers, then one row per message holding one cell per column.
///
/// This is the shape the floor's item view builds from its model, and the shape
/// a hand-painted table has to build for itself or it has none. Measured at
/// 6.11.1, an emptied item view still answers `role = Table` with no diagnostic;
/// here the same emptiness would be `scene/conform`'s `empty` arm unless the
/// grid declared its row count to be zero.
fn list_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let selected = state.row.get();
    let columns = spec::COLUMNS.len();
    // `aria-rowcount` is the total, so the header row is counted — the reading
    // WAI-ARIA states and the one this tree's chart tables already use.
    // ★★★★★ R1694 — built by [`grid_table_nodes`] rather than by hand. This
    // was the hand-rolled copy, and the sibling dashboard screen wrote a second
    // one; the two disagreed about where the header row sits. WAI-ARIA counts
    // it in `aria-rowcount`, so it has to be counted in `aria-rowindex` too,
    // and this copy counted it in one and not the other: sixteen messages
    // numbered one to sixteen out of seventeen, a header with no index at all,
    // and the first message standing where the header belongs. The rule now
    // lives once, in the builder, and the row's name-from-contents with it.
    let grid_columns: Vec<GridColumn> = (0..columns)
        .map(|n| GridColumn {
            tag: format!("pv.list.head.{n}"),
            sort: None,
        })
        .collect();
    let grid_rows: Vec<GridRow> = spec::ROWS
        .iter()
        .enumerate()
        .map(|(n, message)| GridRow {
            tag: format!("pv.list.row.{n}"),
            selected: n == selected,
            state: RadioState::Idle,
            cells: row_cells(message)
                .into_iter()
                .enumerate()
                .map(|(c, text)| GridCell {
                    tag: list_cell_tag(n, c),
                    name: text,
                    focused: false,
                    selected: None,
                })
                .collect(),
        })
        .collect();
    grid_table_nodes(
        "pv.list",
        spec::PANES[0].title,
        false,
        LIST_HEADER,
        &grid_columns,
        &grid_rows,
    )
}

/// The tag the header row is announced under. Nothing paints it — the seven
/// column headers are painted individually and the row is what a reader
/// descends through — so it is anchored by the members it composes.
const LIST_HEADER: &str = "pv.list.header";

/// What one message's cells **paint**, left to right — one entry per
/// [`spec::COLUMNS`] entry.
///
/// The painter's own source, so a column that changes what it shows changes it
/// once. The name column holds the resource name only; the annotations painted
/// beside it in that column are separate runs, placed from the right edge.
fn cell_texts(message: &spec::RowSpec) -> Vec<String> {
    vec![
        message.time.to_owned(),
        message.hop.to_owned(),
        message.channel.to_owned(),
        message.sn.to_string(),
        message.kind.to_owned(),
        message.name.to_owned(),
        message.len.to_string(),
    ]
}

/// What one message's cells **announce**.
///
/// [`cell_texts`] plus the runs painted beside them, so the two cannot drift on
/// the six plain columns and the seventh's difference is stated exactly here.
/// The name column announces all three of its runs because all three are
/// painted in that column, and a reader told only the first would never learn a
/// piece was dropped.
fn row_cells(message: &spec::RowSpec) -> Vec<String> {
    let mut cells = cell_texts(message);
    let name = &mut cells[NAME_COLUMN];
    if let Some(fragment) = &message.fragment {
        name.push(' ');
        name.push_str(fragment.marker);
        name.push(' ');
        name.push_str(fragment.piece);
    }
    if !message.note.is_empty() {
        name.push(' ');
        name.push_str(message.note);
    }
    cells
}

/// The column the resource name and its annotations share.
const NAME_COLUMN: usize = 5;

/// ★★★★★ R1693 — the decode as a **tree**: one item per visible field, carrying
/// its depth, its position among its siblings, and **its value**.
///
/// The value is where this beats the floor rather than matching it. Built and
/// run at 6.11.1, a two-column tree announces a row as **two sibling items** —
/// `L1 transport` and `v0x09` are peers, the value reports `expandable = 1` like
/// the field it belongs to, and leaf rows report `expanded = 1` while reporting
/// they cannot expand. The hierarchy is gone too: every item is a direct child
/// of the tree whatever its depth. Here a field is one item, its value is its
/// value, and `aria-level` carries the depth the paint indents by.
fn tree_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let selected = state.field.get();
    let folded = state.folded.get();
    let visible = visible_fields(state);
    let mut tree = AccessNode::new("pv.tree", AriaRole::Tree)
        // The pane paints its own title and that run declares itself this
        // node's name, so the redirect is true rather than merely well formed.
        .with_name_from_tag("pv.tree.title")
        .with_size_of_set(u32::try_from(visible.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (n, (path, name, value, depth)) in visible.iter().enumerate() {
        let tag = format!("pv.tree.field.{path}");
        tree = tree.with_child(tag.clone());
        let (position, siblings) = sibling_place(&visible, n);
        let mut item = AccessNode::new(tag, AriaRole::TreeItem)
            .with_name(name.clone())
            .with_value(AccessValue::Text(value.clone()))
            .with_level(u32::try_from(*depth).unwrap_or(0) + 1)
            .with_set_position(position, siblings)
            .with_selected(*path == selected);
        // A layer heading folds; a field does not. `aria-expanded` is the state
        // the chevron draws, which is why the chevron itself is declared part of
        // this item rather than announced as a glyph of its own.
        if let Some(index) = spec::LAYERS.iter().position(|(id, _)| id == path) {
            item = item.with_expanded(!folded.get(index).copied().unwrap_or(false));
        }
        nodes.push(item);
    }
    nodes.insert(0, tree);
    nodes
}

/// Where the `n`-th visible field sits **among its own siblings**, and how many
/// of them there are.
///
/// `aria-posinset` counts within a level, not within the flattened list the tree
/// is painted as — a field announced as "3 of 24" when it is the third of four
/// under its layer tells a reader the wrong shape.
///
/// ★ The block is the run between the nearest shallower entries either side, and
/// the count inside it is of entries at **this** depth. The first draft returned
/// the block's whole length, which is right for a layer's children and wrong for
/// the layers themselves: a top-level heading counted every row in the tree. The
/// test written for this function is what said so — it had none for an hour, and
/// that is the shape R1692 measured five times in one round.
fn sibling_place(visible: &[(String, String, String, usize)], n: usize) -> (usize, usize) {
    let depth = visible[n].3;
    let start = visible[..n]
        .iter()
        .rposition(|(_, _, _, d)| *d < depth)
        .map_or(0, |index| index + 1);
    let end = visible[start..]
        .iter()
        .position(|(_, _, _, d)| *d < depth)
        .map_or(visible.len(), |offset| start + offset);
    let at_depth = |slice: &[(String, String, String, usize)]| {
        slice.iter().filter(|(_, _, _, d)| *d == depth).count()
    };
    (at_depth(&visible[start..n]), at_depth(&visible[start..end]))
}

/// ★★★★ R1693 — the bytes as a **grid** of nine columns: the offset as each
/// row's header, then eight cells.
///
/// This is the pane the floor cannot express at all. Measured at 6.11.1, a
/// custom-painted 72-cell widget answers **one** node, empty-named, with no
/// children — because everything there is derived from a model, and a pane that
/// paints itself has no model to derive from. A cell in an item view does report
/// its `rowHeaderCells`, which is why the offsets are headers here rather than
/// decoration.
fn bytes_nodes(state: &Rc<ViewState>) -> Vec<AccessNode> {
    let buffer = state.frame_bytes();
    let layout = hex_layout();
    let lit = state.lit_selection();
    let per_row = spec::BYTES_PER_ROW;
    let mut grid = AccessNode::new("pv.bytes", AriaRole::Grid)
        .with_name_from_tag("pv.bytes.title")
        // The readout beside the title says which bytes the open field covers;
        // it is the grid's description rather than a member of it, because a
        // `grid` owns rows and nothing else.
        .with_described_by("pv.bytes.span")
        .with_row_count(u32::try_from(layout.rows()).unwrap_or(u32::MAX))
        .with_column_count(u32::try_from(per_row).unwrap_or(u32::MAX) + 1);
    let mut nodes = vec![AccessNode::new("pv.bytes.span", AriaRole::Status)];
    for r in 0..layout.rows() {
        let row_tag = bytes_row_tag(r);
        grid = grid.with_child(row_tag.clone());
        let offset = bytes_offset_tag(r);
        let mut row = AccessNode::new(row_tag, AriaRole::Row)
            .with_row(r)
            .with_child(offset.clone());
        nodes.push(
            AccessNode::new(offset, AriaRole::RowHeader)
                .with_row(r)
                .with_column(0),
        );
        for c in 0..per_row {
            let byte = r * per_row + c;
            let Some(value) = buffer.get(byte) else {
                break;
            };
            let tag = format!("pv.bytes.cell.{byte}");
            row = row.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::GridCell)
                    .with_name(format!("{value:02x}"))
                    .with_row(r)
                    .with_column(c + 1)
                    .with_selected(lit.is_some_and(|sel| sel.contains(byte))),
            );
        }
        nodes.push(row);
    }
    nodes.insert(0, grid);
    nodes
}

/// The reassembly strip: one lane per channel carrying traffic, and the totals.
fn reassembly_nodes() -> Vec<AccessNode> {
    let mut group = AccessNode::new("pv.reassembly", AriaRole::Group)
        .with_name_from_tag("pv.reassembly.title")
        .with_child("pv.reassembly.counts");
    let mut nodes = vec![AccessNode::new("pv.reassembly.counts", AriaRole::Status)];
    for (n, lane) in spec::LANES.iter().enumerate() {
        let tag = format!("pv.reassembly.lane.{n}");
        group = group.with_child(tag.clone());
        // The lane paints its name and its continuity as siblings of its box, so
        // both come from the table the painter reads.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(lane.name)
                .with_value(AccessValue::Text(lane_reading(lane))),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// What one reassembly lane reads as — its sequence number, and whether the
/// sequence is unbroken.
fn lane_reading(lane: &spec::LaneSpec) -> String {
    if lane.continuous {
        format!("{} · unbroken", lane.sn)
    } else {
        format!("{} · {} abandoned", lane.sn, lane.dropped)
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
