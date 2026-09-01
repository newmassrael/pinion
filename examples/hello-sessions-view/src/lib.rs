// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-sessions-view` — R1948 §5.27 §5.40 §5.41 — the analysis tool's
//! **sessions section**, the eighth and last seat of the reference's rail.
//!
//! ## What forced this example
//!
//! `docs/analyzer-rail-spec.json`'s `second_phase_owed` has carried exactly one
//! entry since R1947 built `topology`: this seat. The SCOPE mockup draws it
//! locked under requirement 18; the BEHAVIOUR reference builds it, and a person
//! asked for both of these sections by name on 2026-09-01.
//!
//! ## The screen
//!
//! ```text
//! cargo run -p hello-sessions-view --release
//! ```
//!
//! A session list under a 46-high header — the section's name, a derived count,
//! a filter field and three status chips — laid out on the reference's own
//! eight-column grid; beside it a 320-wide detail with a headline, a status
//! pill, the peer it reaches, four negotiated tiles, a handshake timeline, a
//! per-channel sequence list and two actions.
//!
//! Click a row to inspect it; click a chip to keep one standing; the arrows
//! walk the list.
//!
//! ## Three things here are this section's own
//!
//! * ★★★★★ **The peer column is a JOIN, not a label.** Every session names one
//!   of the six peers `hello-topology-view` plots, and `Show in topology` is
//!   the reference's own control across that join — the first cross-section
//!   action in this application, working rather than refusing, because R1947
//!   built the section it leads to. The containment is asserted against
//!   `hello_topology_view::peers()` rather than left to two tables staying in
//!   step by hand.
//! * ★ **The handshake timeline is DERIVED from the standing.** The reference
//!   builds it the same way — four steps every session takes, then a fifth that
//!   depends on how it is doing — and storing it per row would let a session
//!   claim a handshake its own status contradicts.
//! * ★ **The header's count is derived through a predicate on the state**
//!   (`Standing::is_active`), so *active* is a property of a state rather than
//!   a number somebody typed beside a list it does not come from.

mod judge;
mod spec;

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use pinion_a11y::{AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::describe::Descriptions;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, PointerTarget, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextOverflow, TextStyle};
use pinion_core::theme::use_theme;
use pinion_core::voice::Silence;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloSessionsViewRenderer, HelloSessionsViewRendererError);

// ── Tags ────────────────────────────────────────────────────────────────────

/// The screen's paint-root tag, and the tag its external is keyed by.
const VIEW_TAG: &str = "sessions_view";

/// The root group every mark of this screen hangs under.
const ROOT_TAG: &str = "sv.root";

/// The theme this screen reads its palette from.
const THEME_TAG: &str = "app";

/// The list pane's own group.
const LIST_TAG: &str = "sv.list";
/// The detail pane's own group.
const DETAIL_TAG: &str = "sv.detail";
/// The grid of rows, which is the list's own scrollable body.
const ROWS_TAG: &str = "sv.list.rows";
/// Where a resting description is painted and announced.
const TOOLTIP_TAG: &str = "sv.tip";

/// The word the detail's peer line opens with.
///
/// ★★★★★ R1952 — it was `U+2192` RIGHTWARDS ARROW, and the face this tree
/// ships has no glyph for it: `NotoSans-Regular` is a Latin/Greek/Cyrillic text
/// face and arrows live in separate symbol faces. A reader arriving at this
/// section saw a `.notdef` box where the line begins. A mark inside a sentence
/// cannot be a drawn path — it has to flow with the words around it — so the
/// repair is prose, in characters the face can set. It also reads without
/// decoding, which the arrow never did.
///
/// A `const` because the paint and the check that finds the line must not
/// spell it twice.
pub const PEER_LEAD: &str = "to";

// ── Geometry ────────────────────────────────────────────────────────────────

const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;
const DETAIL_W: u32 = spec::DETAIL_W;
const HEADER_H: u32 = spec::HEADER_H;
const COLHEAD_H: u32 = spec::COLHEAD_H;
const ROW_H: u32 = spec::ROW_H;
const PAD: u32 = spec::PAD;

const FONT_TINY: u32 = spec::FONT_TINY;
const FONT_SMALL: u32 = spec::FONT_SMALL;
const FONT_BODY: u32 = spec::FONT_BODY;
const FONT_TITLE: u32 = spec::FONT_TITLE;
const FONT_HEADLINE: u32 = spec::FONT_HEADLINE;

/// ★★★★★ R1948 — **the narrowest this section can be drawn, DERIVED from the
/// grid it has to show.**
///
/// The detail at its declared width, plus every column at its own minimum, plus
/// the gaps, plus the padding. ⚠ The first draft wrote `DETAIL_W + 700` and the
/// sweep at that floor reported the status column running off the pane — 700
/// was a guess and the columns need 774. A floor picked by hand is wrong in
/// exactly one direction, and it is the direction that ships a broken screen.
const fn min_width() -> u32 {
    let mut columns = 0;
    let mut count = 0;
    let mut n = 0;
    while n < spec::COLUMNS.len() {
        // A stretching column contributes its MINIMUM here, which is what the
        // reference's `minmax(110px, 1fr)` says the track may shrink to.
        columns += spec::COLUMNS[n].width;
        count += 1;
        n += 1;
    }
    DETAIL_W + columns + (count - 1) * spec::GAP + PAD * 2
}

/// ★★★★★ R1948 — **the shortest, derived from the detail's own stack.**
///
/// The same defect the other way: the first draft's `520` put the action row
/// ABOVE the channel list at the floor, and the conformance reader — which
/// orders parts by where they are — reported two parts swapped. A height
/// computed from the bands cannot do that.
const fn min_height() -> u32 {
    channels_top() + BAND_HEAD_H + channel_rows() * CHANNEL_PITCH + 34 + PAD * 2
}

const MIN_W: u32 = min_width();
const MIN_H: u32 = min_height();

const _: () = assert!(
    MIN_W < WIN_W && MIN_H < WIN_H,
    "a floor at or above the opening size is not a floor"
);

/// ★ What this screen needs and what it gives up when it does not get it.
///
/// Panning: a session list that dropped columns when the window narrowed would
/// be a list that lies about the capture, and the reference's grid gives one
/// column the slack rather than hiding any.
const SHRINK: ShrinkPolicy = ShrinkPolicy::panning((MIN_W, MIN_H), (760, 460));

/// The extent this section is laying out in.
///
/// ⚠ Read from the framework, never assumed to be [`WIN_W`] x [`WIN_H`] — the
/// defect R1947's sweep caught at the first size that was not the declared
/// pair.
fn window_size() -> (u32, u32) {
    pinion_core::external::layout_size(VIEW_TAG, SHRINK.comfortable(), (WIN_W, WIN_H))
}

/// The detail pane.
fn detail_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(w.saturating_sub(DETAIL_W), 0, DETAIL_W, h)
}

/// The list pane — everything left of the detail.
fn list_rect() -> Rect {
    let (w, h) = window_size();
    Rect::new(0, 0, w.saturating_sub(DETAIL_W).max(1), h)
}

/// The list's header strip.
fn list_header_rect() -> Rect {
    let list = list_rect();
    Rect::new(list.x, list.y, list.w, HEADER_H)
}

/// The column-heading strip under it.
fn colhead_rect() -> Rect {
    let list = list_rect();
    Rect::new(list.x, list.y + HEADER_H, list.w, COLHEAD_H)
}

/// The scrollable body of rows.
fn rows_rect() -> Rect {
    let list = list_rect();
    let top = HEADER_H + COLHEAD_H;
    Rect::new(list.x, list.y + top, list.w, list.h.saturating_sub(top))
}

/// One session row, by its index among the rows currently kept.
fn row_rect(visual: usize) -> Rect {
    let body = rows_rect();
    let nth = u32::try_from(visual).unwrap_or(0);
    Rect::new(body.x, body.y + nth * ROW_H, body.w, ROW_H)
}

/// Where a column's cells sit inside a row.
fn column_rect(nth: usize) -> Rect {
    spec::column_rect(nth, list_rect())
}

/// The filter field in the list header.
fn filter_rect() -> Rect {
    let head = list_header_rect();
    let wide = 200;
    Rect::new(
        head.x + head.w.saturating_sub(PAD + chips_width() + 12 + wide),
        head.y + (HEADER_H - 32) / 2,
        wide,
        32,
    )
}

/// How wide the row of status chips is, all told.
fn chips_width() -> u32 {
    let each = 96;
    let gaps = u32::try_from(spec::CHIPS.len().saturating_sub(1)).unwrap_or(0) * 6;
    u32::try_from(spec::CHIPS.len()).unwrap_or(0) * each + gaps
}

/// One status chip.
fn chip_rect(nth: u32) -> Rect {
    let head = list_header_rect();
    let each = 96;
    Rect::new(
        head.x + head.w.saturating_sub(PAD + chips_width()) + nth * (each + 6),
        head.y + (HEADER_H - 26) / 2,
        each,
        26,
    )
}

/// A band inside the detail pane, in the PANE's own coordinates.
///
/// ⚠ Pane-local, and every caller keeps it that way. R1947's key band mixed the
/// window's `y` with a part box's own space and painted six marks up to 270px
/// below the box that owns them.
fn detail_band(top: u32, height: u32) -> Rect {
    let pane = detail_rect();
    Rect::new(PAD, top, pane.w.saturating_sub(PAD * 2), height)
}

/// One negotiated tile of the four.
fn tile_rect(nth: u32) -> Rect {
    let pane = detail_rect();
    let wide = (pane.w.saturating_sub(PAD * 2 + 9)) / 2;
    Rect::new(
        PAD + (nth % 2) * (wide + 9),
        NEGOTIATED_TOP + (nth / 2) * (spec::TILE_H + 9),
        wide,
        spec::TILE_H,
    )
}

/// Where the negotiated block starts in the detail — under the identity block,
/// which is the one band whose height is fixed by what it holds.
const NEGOTIATED_TOP: u32 = 176;
/// How far apart two timeline steps are.
const TIMELINE_PITCH: u32 = 26;
/// How far apart two channel rows are.
const CHANNEL_PITCH: u32 = 30;
/// The room a band's own heading takes above its first row.
const BAND_HEAD_H: u32 = 24;
/// The room between one band and the next.
const BAND_GAP: u32 = 26;

/// ★★★★★ R1948 — **each band starts where the one above it ends.**
///
/// Derived rather than tabled, which is the rule R1947 reached for the filter
/// rail's groups and this round needed for the same reason one round later: a
/// hand-written table of tops is correct exactly until a band gains a row, and
/// then it overlaps silently. Here a longer handshake pushes the channels down
/// and the floor with them.
const fn timeline_top() -> u32 {
    NEGOTIATED_TOP + 2 * (spec::TILE_H + 9) + BAND_GAP
}

/// Where the channel list starts.
const fn channels_top() -> u32 {
    timeline_top() + BAND_HEAD_H + spec::MAX_HANDSHAKE_STEPS * TIMELINE_PITCH + BAND_GAP
}

/// How many channel rows the detail draws.
const fn channel_rows() -> u32 {
    let mut count = 0;
    let mut n = 0;
    while n < spec::CHANNELS.len() {
        count += 1;
        n += 1;
    }
    count
}

/// One of the detail's two actions, **in the detail pane's own space** — which
/// is the space the pane's children are laid out in, and the space
/// [`action_row`] paints them at.
fn action_rect(nth: u32) -> Rect {
    let pane = detail_rect();
    let (_, h) = window_size();
    let wide = pane.w.saturating_sub(PAD * 2 + 8);
    // The reference gives the crossing action the slack and the destructive one
    // a fixed square, which is what says which of the two is the ordinary
    // gesture without either carrying a word about it.
    if nth == 0 {
        Rect::new(PAD, h.saturating_sub(PAD + 34), wide.saturating_sub(44), 34)
    } else {
        Rect::new(
            PAD + wide.saturating_sub(44) + 8,
            h.saturating_sub(PAD + 34),
            44,
            34,
        )
    }
}

/// The same action, **as the window sees it**.
///
/// ★★★★★ R1953 — the defect this exists to remove: [`Hit::at`] works in the
/// SCREEN's coordinates (every other rectangle it tests — a chip, a row — is
/// measured from the window's left edge) and it was testing [`action_rect`],
/// which is measured from the detail PANE's. The pane starts at
/// `w - DETAIL_W`, so the two frames differ by that much in `x` and agree in
/// `y` — which is why the mistake survived: the buttons were dead to a real
/// mouse, and a press in the LIST pane's bottom-left corner reached them
/// instead.
///
/// ⚠ It survived R1948's own gates because every assertion about these two
/// actions addressed them BY NAME over the wire, and the wire path never asks
/// the geometry. `scene/pointer_target` is what asks, and it reported both rows
/// `astray` the first time this example was measured — which only happened at
/// all because R1953 registered its missing census rows.
///
/// One translation, in one direction, at the one place the frame changes. The
/// paint keeps the pane's frame, because that is the frame the pane's children
/// are laid out in; R1948's first draft tried to translate on the paint side
/// and wrote `close.x - pane.x + pane.x`, a no-op that also underflowed.
fn action_in_window(nth: u32) -> Rect {
    let pane = detail_rect();
    let local = action_rect(nth);
    Rect::new(pane.x + local.x, pane.y + local.y, local.w, local.h)
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
    ground: Color,
    surface: Color,
    raised: Color,
    border: Color,
    edge: Color,
    text: Color,
    dim: Color,
    faint: Color,
    accent: Color,
    accent_fg: Color,
    accent_soft: Color,
    ok: Color,
    warn: Color,
    err: Color,
}

fn ink() -> Ink {
    if use_theme(THEME_TAG).is_dark() {
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
            accent_soft: rgb(0x1E_2A_44),
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
            accent_soft: rgb(0xE4_ED_FC),
            ok: rgb(0x1F_8A_4C),
            warn: rgb(0xA9_66_00),
            err: rgb(0xC2_35_45),
        }
    }
}

/// The colour a standing is drawn in.
fn standing_ink(standing: spec::Standing, ink: Ink) -> Color {
    match standing {
        spec::Standing::Established => ink.ok,
        spec::Standing::Reconnecting => ink.warn,
        spec::Standing::Closed => ink.err,
    }
}

/// ★★★★★ R1948 — **the colour a graded word is drawn in, chosen by its RANK in
/// the scale rather than by matching the word.**
///
/// The distinction is the escape hatch it removes. A `match` on the string
/// needs a `_` arm, and that arm is where a word the vocabulary does not hold
/// gets a colour anyway — drawn, plausible, and saying something the scale never
/// agreed to. Asking for the rank means an ungraded word has no colour to fall
/// back on, so it PANICS, and `tests::r1948_every_standing_is_graded_by_the_
/// scale_this_application_uses` is what keeps that unreachable.
///
/// # Panics
///
/// If `severity` is not a word of [`spec::SEVERITY`] — a defect in the tables
/// rather than a state the screen can reach.
fn severity_ink(severity: &str, ink: Ink) -> Color {
    let rank = spec::SEVERITY
        .rank(severity)
        .unwrap_or_else(|| panic!("{severity:?} is not a word this application grades by"));
    match rank {
        0 => ink.accent_fg,
        1 => ink.warn,
        _ => ink.err,
    }
}

// ── State ───────────────────────────────────────────────────────────────────

/// What this section holds between frames.
#[derive(Debug)]
struct ViewState {
    /// The session the detail is showing.
    selected: Signal<String>,
    /// Which of [`spec::CHIPS`] is keeping the list.
    chip: Signal<usize>,
    /// Whether a pointer is over this screen at all.
    pointer_inside: Signal<bool>,
    /// Where the pointer is resting, when it is inside.
    resting: Signal<Option<String>>,
    /// What the section last said about itself.
    said: Signal<String>,
    /// Where a crossing action asked the host to go, if anywhere.
    ///
    /// ★★★★★ The section does not NAVIGATE — it cannot, and should not: a page
    /// that moved the host's rail would be a page deciding what application it
    /// is in. It publishes the request, the host reads it, and a standalone
    /// window simply has nobody listening. That is the same shape every other
    /// cross-screen fact in this tree takes.
    crossing: Signal<Option<String>>,
}

impl ViewState {
    fn new() -> Self {
        Self {
            selected: Signal::new(spec::OPENS_ON.to_owned()),
            chip: Signal::new(0),
            pointer_inside: Signal::new(false),
            resting: Signal::new(None),
            said: Signal::new(String::new()),
            crossing: Signal::new(None),
        }
    }

    /// The session the detail is showing. Never `None`: the section opens on
    /// one and every way of changing it picks another.
    fn picked(&self) -> &'static spec::SessionSpec {
        spec::session(&self.selected.get()).unwrap_or(&spec::SESSIONS[0])
    }

    /// The chip in force.
    fn chip(&self) -> &'static spec::ChipSpec {
        let nth = self.chip.get().min(spec::CHIPS.len() - 1);
        &spec::CHIPS[nth]
    }

    /// The sessions the list is showing, in the table's order.
    fn kept(&self) -> Vec<&'static spec::SessionSpec> {
        let keeps = self.chip().keeps;
        spec::SESSIONS
            .iter()
            .filter(|s| keeps.is_none_or(|standing| s.standing == standing))
            .collect()
    }

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

/// What is under a point.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    /// A session row, by its id.
    Row(&'static str),
    /// A status chip, by its index in [`spec::CHIPS`].
    Chip(usize),
    /// The action that crosses to the topology section.
    Cross,
    /// The action that refuses.
    Close,
    /// Nothing pressable.
    Nothing,
}

impl Hit {
    /// What is at a point of this screen's own space.
    fn at(state: &Rc<ViewState>, px: u32, py: u32) -> Self {
        for (n, _) in spec::CHIPS.iter().enumerate() {
            if contains(chip_rect(u32::try_from(n).unwrap_or(0)), px, py) {
                return Self::Chip(n);
            }
        }
        if contains(action_in_window(0), px, py) {
            return Self::Cross;
        }
        if contains(action_in_window(1), px, py) {
            return Self::Close;
        }
        for (visual, session) in state.kept().iter().enumerate() {
            if contains(row_rect(visual), px, py) {
                return Self::Row(session.id);
            }
        }
        Self::Nothing
    }

    /// What is under a paint tag — the same answer by another address.
    fn of_tag(tag: &str) -> Self {
        // ★ A cell's tag is `sv.row.<id>.<column>`, so the id is the first
        // segment — pressing a cell is pressing its row, which is what a reader
        // means by it and what the pointer already does by coordinate.
        if let Some(rest) = tag.strip_prefix("sv.row.")
            && let Some(session) = spec::session(rest.split('.').next().unwrap_or(rest))
        {
            return Self::Row(session.id);
        }
        if let Some(key) = tag.strip_prefix("sv.chip.")
            && let Some(n) = spec::CHIPS.iter().position(|c| c.key == key)
        {
            return Self::Chip(n);
        }
        match tag {
            "sv.detail.topology" => Self::Cross,
            "sv.detail.close" => Self::Close,
            _ => Self::Nothing,
        }
    }

    /// The word this hit answers on the wire.
    fn word(&self) -> Option<String> {
        match self {
            Self::Row(id) => Some(format!("session:{id}")),
            Self::Chip(n) => Some(format!("chip:{}", spec::CHIPS[*n].key)),
            Self::Cross => Some("topology".to_owned()),
            Self::Close => Some("close".to_owned()),
            Self::Nothing => None,
        }
    }
}

// ── The handlers a press and the wire both reach ────────────────────────────

/// Pick a session.
fn select_session(state: &Rc<ViewState>, id: &str) {
    let Some(session) = spec::session(id) else {
        return;
    };
    state.selected.set(session.id.to_owned());
    state.say(format!("{} selected", session.id));
}

/// Keep one standing.
///
/// ★ The selection follows: a detail describing a row the list no longer holds
/// is a panel about something a reader cannot see. If the picked session is
/// filtered away the first kept one takes over, and the section SAYS so.
fn choose_chip(state: &Rc<ViewState>, nth: usize) {
    state.chip.set(nth.min(spec::CHIPS.len() - 1));
    let chip = state.chip();
    let kept = state.kept();
    let still_there = kept.iter().any(|s| s.id == state.selected.get());
    if !still_there && let Some(first) = kept.first() {
        state.selected.set(first.id.to_owned());
    }
    state.say(format!("{} \u{00B7} {} shown", chip.title, kept.len()));
}

/// Ask the host to show the picked session's peer in the topology section.
fn cross_to_topology(state: &Rc<ViewState>) {
    let session = state.picked();
    state.crossing.set(Some(session.peer.to_owned()));
    state.say(format!("show {} in topology", session.peer));
}

/// The destructive action, drawn and refused.
fn refuse_close(state: &Rc<ViewState>) {
    state.say(format!(
        "Close session is not in this release - booked under {}",
        spec::CLOSE_RESERVED_FOR
    ));
}

/// Apply a hit, wherever it came from.
fn press(state: &Rc<ViewState>, hit: &Hit) -> bool {
    match hit {
        Hit::Row(id) => {
            select_session(state, id);
            true
        }
        Hit::Chip(n) => {
            choose_chip(state, *n);
            true
        }
        Hit::Cross => {
            cross_to_topology(state);
            true
        }
        Hit::Close => {
            refuse_close(state);
            true
        }
        Hit::Nothing => false,
    }
}

/// Walk the list from the keyboard.
fn key_at(state: &Rc<ViewState>, chord: &str) -> bool {
    let kept = state.kept();
    if kept.is_empty() {
        return false;
    }
    let here = kept
        .iter()
        .position(|s| s.id == state.selected.get())
        .unwrap_or(0);
    let last = kept.len() - 1;
    let next = match chord {
        "ArrowDown" | "ArrowRight" => {
            if here == last {
                0
            } else {
                here + 1
            }
        }
        "ArrowUp" | "ArrowLeft" => {
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
    select_session(state, kept[next].id);
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

/// A seat for one line of `px` type — box and run the same height, derived.
fn seat(x: u32, y: u32, w: u32, px: u32) -> Rect {
    pinion_core::containment::line_rect(x, y, w, px)
}

/// ★★★★★ R1956 — **the band one row of a detail list occupies**, which is what
/// [`seat`] cannot be: `seat` takes a TOP and hands back a box as tall as the
/// face, so two faces given the same top are top-aligned and their centre lines
/// are a pixel apart whenever their line boxes differ in parity.
///
/// Measured on this view: `sv.detail.timeline` and `sv.detail.channels` put a
/// [`FONT_BODY`] run and a [`FONT_SMALL`] one on every row from one `top`, and
/// the gate `containment::uncentred` reported **eleven** pairs standing beside
/// each other one pixel out. That is R1862's defect — a legend's pin and label,
/// each given a plausible offset that nothing related — in a second place.
///
/// The row is as tall as the **largest** face it holds, so the run that sets
/// the row's height keeps exactly the rectangle it had and only the smaller
/// ones move onto its centre line.
fn row_band(y: u32, w: u32, tallest: u32) -> Rect {
    Rect::new(0, y, w, pinion_core::containment::line_box(tallest))
}

/// The nth handshake-timeline row's band.
fn timeline_row(w: u32, n: usize) -> Rect {
    let top = BAND_HEAD_H + u32::try_from(n).unwrap_or(0) * TIMELINE_PITCH;
    row_band(top, w, FONT_BODY)
}

/// The nth channel row's band.
fn channel_row(w: u32, n: usize) -> Rect {
    let top = BAND_HEAD_H + u32::try_from(n).unwrap_or(0) * CHANNEL_PITCH;
    row_band(top, w, FONT_BODY)
}

/// A run's box inside a row's band, centred on the band's line rather than
/// hung from its top. [`seat`]'s counterpart for anything that shares a row.
fn in_row(row: Rect, x: u32, w: u32, px: u32) -> Rect {
    pinion_core::containment::line_rect_in(row, x, w, px)
}

/// ★ A run's box is as tall as its own face, always — the height the caller
/// passes is DISCARDED. R1947's first sweep reported 53 of 57 runs in a box too
/// short for their letters, which is one convention rather than 53 slips.
fn label(text: impl Into<String>, rect: Rect, px: u32, fg: Color) -> Scene {
    let box_rect = seat(rect.x, rect.y, rect.w, px);
    Scene::Text(
        TextNode::styled(text.into(), box_rect, run_style(px, fg)).with_layout(absolute(box_rect)),
    )
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

/// A drawn box carrying no words of its own.
///
/// `None` means the box speaks through an `AccessNode` with a name — a
/// different and stronger claim than being silent, and one the census can tell
/// apart.
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
    match silence {
        Some(why) => node.silenced(why),
        None => node,
    }
}

/// ★ A box and the words in it, BOUND rather than adjacent — the relation the
/// shell's caption ratchet counts. A box and a label pushed as siblings are
/// paired by where they landed, and R1947 took the application from 148 such
/// pairs to 173 before this helper existed.
fn captioned_box(
    tag: &str,
    rect: Rect,
    fill: Color,
    border: Option<Color>,
    radius: u32,
    words: (&str, u32, Color),
    silence: Option<Silence>,
) -> Scene {
    bound_run(tag, rect, fill, border, radius, words, silence, true)
}

/// The same, left-aligned — for a run that reads as a line of the pane rather
/// than as a control's label.
///
/// ⚠ Two entry points rather than a `centred: bool` at every call: alignment is
/// a property of what the run IS, and the two kinds are not interchangeable. A
/// heading centred in its band would be a visual regression that the caption
/// ratchet reports as progress, which is the trade this split refuses.
fn bound_line(tag: &str, rect: Rect, words: (&str, u32, Color), silence: Option<Silence>) -> Scene {
    bound_run(
        tag,
        seat(rect.x, rect.y, rect.w, words.1),
        Color::rgba(0, 0, 0, 0),
        None,
        0,
        words,
        silence,
        false,
    )
}

/// The one place a caption is bound to a box in this crate.
#[allow(clippy::too_many_arguments)]
fn bound_run(
    tag: &str,
    rect: Rect,
    fill: Color,
    border: Option<Color>,
    radius: u32,
    words: (&str, u32, Color),
    silence: Option<Silence>,
    centred: bool,
) -> Scene {
    let mut style = BoxStyle::filled(fill).with_corner_radius(radius);
    if let Some(colour) = border {
        style = style.with_border(Border::new(colour, FRAME));
    }
    let (text, px, fg) = words;
    // The run answers to whatever the box answers to: they are one thing to a
    // reader. `name_of` and a bare `part_of(tag)` both left the run dangling
    // when the box was itself only a part of something else (R1947 measured
    // both).
    let voice = silence
        .clone()
        .unwrap_or_else(|| Silence::part_of(tag.to_owned()));
    let mut caption = pinion_widget_paint::caption::Caption::new(text, run_style(px, fg));
    if centred {
        caption = caption.centred();
    }
    let node = pinion_widget_paint::caption::captioned(
        tag,
        rect,
        style,
        &caption.silent(voice),
        pinion_widget_paint::caption::Pointer::Transparent,
    )
    .0;
    match silence {
        Some(why) => node.silenced(why),
        None => node,
    }
}

/// ★★★★★ R1948 — **a band's own heading, bound to a box of its own size.**
///
/// The caption ratchet pairs a run with *the smallest box whose interior holds
/// its centre*, and a heading drawn straight into a pane gets paired with the
/// PANE — adjacent, by nothing but where it landed. Wrapping it in a box that is
/// exactly its own seat makes the pairing a fact of the scene instead, and the
/// box is transparent so nothing about the drawing changes.
///
/// ⚠ The box is sized to the heading and not to the band, which is what keeps
/// it from becoming the holder of every other run in that band. A repair that
/// moved a pairing rather than declaring it would read as progress on the
/// ratchet and be none.
fn band_heading(tag: &str, rect: Rect, text: &str, ink: Ink) -> Scene {
    bound_line(
        tag,
        rect,
        (text, FONT_TINY, ink.faint),
        Some(Silence::decorative("heads the band under it")),
    )
}

/// One part of a specified surface. NAMED through [`access_nodes`] rather than
/// silenced: a `layout` silence over untagged runs is reported hollow, and
/// correctly so.
fn part_box(tag: &str, rect: Rect, children: Vec<Scene>) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_layout(absolute(rect)),
    )
}

// ── The list ────────────────────────────────────────────────────────────────

fn list_pane(state: &Rc<ViewState>, ink: Ink) -> Vec<Scene> {
    let list = list_rect();
    let head = list_header_rect();
    // ★ Every header part is the full band tall, so the order a reader reads
    // them in is their order across the bar — R1947's four-parts-out-of-place.
    let band = |x: u32, w: u32| Rect::new(x, head.y, w, HEADER_H);
    let centred = |w: u32, px: u32| {
        pinion_core::containment::line_rect_in(Rect::new(0, 0, w, HEADER_H), 0, w, px)
    };
    let (active, closed) = spec::tally();
    let mut out = vec![
        // ★ R1953 — the list pane is the other stop; see the note on the
        // detail pane below for what shipped without either.
        panel(LIST_TAG, list, ink.ground, None, Vec::new()).with_focusable(true),
        part_box(
            "sv.list.title",
            band(head.x + 18, 110),
            vec![label(
                "Sessions",
                centred(110, FONT_TITLE),
                FONT_TITLE,
                ink.text,
            )],
        ),
        part_box(
            "sv.list.count",
            band(head.x + 140, 170),
            vec![label(
                format!("{active} active \u{00B7} {closed} closed"),
                centred(170, FONT_SMALL),
                FONT_SMALL,
                ink.faint,
            )],
        ),
    ];
    let filter = filter_rect();
    out.push(part_box(
        "sv.list.filter",
        band(filter.x, filter.w),
        vec![captioned_box(
            "sv.list.filter.box",
            Rect::new(0, (HEADER_H - filter.h) / 2, filter.w, filter.h),
            ink.raised,
            Some(ink.border),
            8,
            (spec::FILTER_HINT, FONT_SMALL, ink.faint),
            Some(Silence::part_of("sv.list.filter")),
        )],
    ));
    out.push(chips_part(state, ink));
    out.push(column_headings(ink));
    out.push(rows_part(state, ink));
    out
}

/// The three status chips, as the specification's one part.
fn chips_part(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let head = list_header_rect();
    let first = chip_rect(0);
    let whole = Rect::new(first.x, head.y, chips_width(), HEADER_H);
    let mut children = Vec::new();
    for (n, chip) in spec::CHIPS.iter().enumerate() {
        let nth = u32::try_from(n).unwrap_or(0);
        let rect = chip_rect(nth);
        let local = Rect::new(rect.x - whole.x, rect.y - whole.y, rect.w, rect.h);
        let on = state.chip.get() == n;
        children.push(captioned_box(
            &format!("sv.chip.{}", chip.key),
            local,
            if on { ink.accent_soft } else { ink.ground },
            if on { Some(ink.accent) } else { None },
            7,
            (
                chip.title,
                FONT_SMALL,
                if on { ink.accent_fg } else { ink.dim },
            ),
            // A `RadioButton` access node names it and says whether it is the
            // one in force.
            None,
        ));
    }
    part_box("sv.list.chips", whole, children)
}

/// The eight column headings, in the reference's own grid.
fn column_headings(ink: Ink) -> Scene {
    let strip = colhead_rect();
    let mut children = Vec::new();
    for (n, column) in spec::COLUMNS.iter().enumerate() {
        let cell = column_rect(n);
        children.push(label(
            column.title.to_uppercase(),
            pinion_core::containment::line_rect_in(
                Rect::new(cell.x - strip.x, 0, cell.w, COLHEAD_H),
                cell.x - strip.x,
                cell.w,
                FONT_TINY,
            ),
            FONT_TINY,
            ink.faint,
        ));
    }
    part_box("sv.list.columns", strip, children)
}

/// The rows the chip in force keeps.
fn rows_part(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let body = rows_rect();
    let picked = state.picked();
    let mut children = Vec::new();
    for (visual, session) in state.kept().iter().enumerate() {
        let row = row_rect(visual);
        let local = Rect::new(0, row.y - body.y, body.w, ROW_H);
        let on = session.id == picked.id;
        children.push(box_at(
            &format!("sv.row.{}", session.id),
            local,
            if on { ink.accent_soft } else { ink.ground },
            Some(ink.border),
            0,
            // A `Row` access node names the session and says whether it is the
            // one the detail describes.
            None,
        ));
        for (n, column) in spec::COLUMNS.iter().enumerate() {
            let cell = column_rect(n);
            let ink_for = if column.key == "status" {
                standing_ink(session.standing, ink)
            } else if column.key == "session" {
                ink.accent_fg
            } else {
                ink.dim
            };
            // ★★★★★ R1948 — every cell is BOUND to a seat of its own, not drawn
            // into the row and paired with it by geometry.
            //
            // Measured rather than assumed: the section's own caption survey
            // reported exactly 6 adjacent pairs, and they were the six `peer`
            // cells — that column takes the grid's slack, so at the declared
            // width it grows past a quarter of the row's width and the pairing
            // rule claims it. Binding only that column would fix the number at
            // one size and leave it wrong at another, since which column is
            // widest moves with the window. So all eight are bound.
            children.push(bound_line(
                &format!("sv.row.{}.{}", session.id, column.key),
                pinion_core::containment::line_rect_in(
                    Rect::new(cell.x - body.x, row.y - body.y, cell.w, ROW_H),
                    cell.x - body.x,
                    cell.w,
                    FONT_BODY,
                ),
                (session.cell(column.key), FONT_BODY, ink_for),
                Some(Silence::part_of(format!("sv.row.{}", session.id))),
            ));
        }
    }
    part_box(ROWS_TAG, body, children)
}

// ── The detail ──────────────────────────────────────────────────────────────

fn detail_pane(state: &Rc<ViewState>, ink: Ink) -> Scene {
    let pane = detail_rect();
    let session = state.picked();
    let mut children = vec![
        part_box(
            "sv.detail.title",
            seat(PAD, 18, 150, FONT_TITLE),
            vec![label(
                "Session detail",
                Rect::new(0, 0, 150, 0),
                FONT_TITLE,
                ink.text,
            )],
        ),
        part_box(
            "sv.detail.badge",
            Rect::new(pane.w.saturating_sub(PAD + 76), 17, 76, 24),
            vec![captioned_box(
                "sv.detail.badge.box",
                Rect::new(0, 0, 76, 24),
                ink.raised,
                Some(ink.border),
                6,
                (session.id, FONT_SMALL, ink.dim),
                Some(Silence::part_of("sv.detail.badge")),
            )],
        ),
        part_box(
            "sv.detail.id",
            seat(PAD, 64, pane.w.saturating_sub(PAD * 2), FONT_HEADLINE),
            vec![label(
                session.id,
                Rect::new(0, 0, pane.w.saturating_sub(PAD * 2), 0),
                FONT_HEADLINE,
                ink.text,
            )],
        ),
        part_box(
            "sv.detail.status",
            Rect::new(PAD, 100, 132, 24),
            vec![captioned_box(
                "sv.detail.status.pill",
                Rect::new(0, 0, 132, 24),
                ink.raised,
                Some(standing_ink(session.standing, ink)),
                12,
                (
                    session.standing.label(),
                    FONT_SMALL,
                    standing_ink(session.standing, ink),
                ),
                Some(Silence::part_of("sv.detail.status")),
            )],
        ),
        part_box(
            "sv.detail.peer",
            seat(PAD, 136, pane.w.saturating_sub(PAD * 2), FONT_SMALL),
            vec![label(
                format!("{PEER_LEAD} {} \u{00B7} {}", session.peer, session.zid),
                Rect::new(0, 0, pane.w.saturating_sub(PAD * 2), 0),
                FONT_SMALL,
                ink.faint,
            )],
        ),
    ];
    children.extend(negotiated_tiles(session, ink));
    children.push(timeline_part(session, ink));
    children.push(channels_part(session, ink));
    children.extend(action_row(ink));
    // ★★★★★ R1953 — a keyboard stop, and this section shipped without one.
    //
    // Measured: this screen publishes 107 deliverable regions and interactive
    // ARIA roles, and `focus/next` found NO stop at step 0 — announced as
    // operable, unreachable by keyboard. The sibling section
    // (`hello-log-view`) puts one stop on each of its two panes, which is the
    // WAI-ARIA composite pattern the framework's `focus_stop` doc names: the
    // container is the stop and a cursor moves among its members.
    //
    // ⚠ It went unnoticed because the demo that asks — `r1570_1` — is CI's,
    // and CI was red for an unrelated reason for four pushes.
    panel(DETAIL_TAG, pane, ink.surface, Some(ink.border), children).with_focusable(true)
}

/// The four negotiated values, in the two-by-two grid the reference draws.
fn negotiated_tiles(session: &spec::SessionSpec, ink: Ink) -> Vec<Scene> {
    let tiles: [(&str, &str, &str); 4] = [
        ("version", "VERSION", session.version),
        ("batch", "BATCH", session.batch),
        ("resolution", "RESOLUTION", session.resolution),
        ("encryption", "ENCRYPTION", session.encryption),
    ];
    let mut out = Vec::new();
    for (n, (key, heading, value)) in tiles.into_iter().enumerate() {
        let rect = tile_rect(u32::try_from(n).unwrap_or(0));
        out.push(part_box(
            &format!("sv.detail.{key}"),
            rect,
            vec![
                // The VALUE is bound to the tile; the heading above it is a
                // second run, because a box carries one caption.
                captioned_box(
                    &format!("sv.detail.{key}.box"),
                    Rect::new(0, 0, rect.w, rect.h),
                    ink.raised,
                    Some(ink.border),
                    9,
                    (value, FONT_BODY, ink.text),
                    Some(Silence::part_of(format!("sv.detail.{key}"))),
                ),
                band_heading(
                    &format!("sv.detail.{key}.head"),
                    Rect::new(11, 9, rect.w.saturating_sub(22), 0),
                    heading,
                    ink,
                ),
            ],
        ));
    }
    out
}

/// The handshake, derived from the standing.
fn timeline_part(session: &spec::SessionSpec, ink: Ink) -> Scene {
    let steps = session.timeline();
    let band = detail_band(
        timeline_top(),
        BAND_HEAD_H + u32::try_from(steps.len()).unwrap_or(0) * TIMELINE_PITCH,
    );
    let mut children = vec![band_heading(
        "sv.detail.timeline.head",
        Rect::new(0, 0, band.w, 0),
        "HANDSHAKE TIMELINE",
        ink,
    )];
    for (n, step) in steps.iter().enumerate() {
        let row = timeline_row(band.w, n);
        children.push(box_at(
            &format!("sv.detail.timeline.{n}.dot"),
            pinion_core::containment::band_in(row, 0, 9, 9),
            severity_ink(step.severity, ink),
            None,
            4,
            Some(Silence::decorative(
                "grades the step beside it; the step's own run names it",
            )),
        ));
        children.push(bound_line(
            &format!("sv.detail.timeline.{n}"),
            in_row(row, 19, band.w.saturating_sub(19 + 64), FONT_BODY),
            (step.label, FONT_BODY, ink.text),
            Some(Silence::part_of("sv.detail.timeline")),
        ));
        children.push(label(
            step.at,
            in_row(row, band.w.saturating_sub(60), 60, FONT_SMALL),
            FONT_SMALL,
            ink.faint,
        ));
    }
    part_box("sv.detail.timeline", band, children)
}

/// The channels and what each last carried.
fn channels_part(session: &spec::SessionSpec, ink: Ink) -> Scene {
    let band = detail_band(channels_top(), BAND_HEAD_H + channel_rows() * CHANNEL_PITCH);
    let mut children = vec![band_heading(
        "sv.detail.channels.head",
        Rect::new(0, 0, band.w, 0),
        "CHANNELS \u{00B7} LAST SEQUENCE",
        ink,
    )];
    for (n, channel) in spec::CHANNELS.iter().enumerate() {
        let row = channel_row(band.w, n);
        children.push(bound_line(
            &format!("sv.detail.channels.{n}"),
            in_row(row, 0, band.w.saturating_sub(150), FONT_BODY),
            (channel.name, FONT_BODY, ink.text),
            Some(Silence::part_of("sv.detail.channels")),
        ));
        children.push(label(
            channel.reliability,
            in_row(row, band.w.saturating_sub(148), 80, FONT_SMALL),
            FONT_SMALL,
            ink.dim,
        ));
        children.push(label(
            session.sequence(channel),
            in_row(row, band.w.saturating_sub(64), 64, FONT_SMALL),
            FONT_SMALL,
            ink.faint,
        ));
    }
    part_box("sv.detail.channels", band, children)
}

/// The crossing action and the refusing one.
fn action_row(ink: Ink) -> Vec<Scene> {
    // ⚠ Both rectangles are already in the PANE's coordinates — `action_rect`
    // measures from `PAD`, not from the window's left edge. The first draft
    // wrote `close.x - pane.x + pane.x`, which is both a no-op and an underflow
    // the moment the pane does not start at zero; the sweep panicked on it at
    // every size.
    let cross = action_rect(0);
    let close = action_rect(1);
    vec![
        part_box(
            "sv.detail.topology",
            cross,
            vec![captioned_box(
                "sv.detail.topology.box",
                Rect::new(0, 0, cross.w, cross.h),
                ink.accent,
                None,
                8,
                ("Show in topology", FONT_BODY, ink.surface),
                Some(Silence::part_of("sv.detail.topology")),
            )],
        ),
        part_box(
            "sv.detail.close",
            close,
            vec![captioned_box(
                "sv.detail.close.box",
                Rect::new(0, 0, close.w, close.h),
                ink.raised,
                Some(ink.err),
                8,
                ("Close", FONT_SMALL, ink.err),
                Some(Silence::part_of("sv.detail.close")),
            )],
        ),
    ]
}

// ── Descriptions ────────────────────────────────────────────────────────────

/// ★★★★★ R1953 — this section's described register, in the shape every OTHER
/// page of this application publishes it in.
///
/// It answered a bare list of tags where the capture viewer, the key-pattern
/// section and the log view all answer
/// `{region, marks: [{tag, sentence}]}` — one channel name meaning two shapes,
/// which is worse than a missing channel because the reader that fails is the
/// one that trusted the name. The sentences are unchanged; only the wire form
/// moves.
fn described_wire() -> serde_json::Value {
    let described = descriptions();
    serde_json::json!({
        "region": TOOLTIP_TAG,
        "marks": described
            .tags()
            .map(|tag| serde_json::json!({
                "tag": tag,
                "sentence": described.of(tag).unwrap_or_default(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// The sentences this screen's marks carry, derived from the declarations they
/// are built from.
fn descriptions() -> Descriptions {
    let mut described = Descriptions::new();
    for chip in spec::CHIPS {
        described.describe(
            format!("sv.chip.{}", chip.key),
            chip.keeps.map_or_else(
                || "Show every session the capture observed".to_owned(),
                |standing| format!("Show only sessions that are {}", standing.label()),
            ),
        );
    }
    for session in spec::SESSIONS {
        described.describe(
            format!("sv.row.{}", session.id),
            format!(
                "{} to {} over {} - {}",
                session.id,
                session.peer,
                session.link,
                session.standing.label()
            ),
        );
    }
    described.describe(
        "sv.detail.topology",
        "Show this session's peer in the topology section",
    );
    described.describe(
        "sv.detail.close",
        format!(
            "Close session is not in this release - booked under {}",
            spec::CLOSE_RESERVED_FOR
        ),
    );
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
    let body = rows_rect();
    Rect::new(body.x + PAD, body.y + PAD, 320, 22)
}

// ── The view ────────────────────────────────────────────────────────────────

fn view(_state: (), _frame: Frame) -> Scene {
    let state = use_view_state();
    let ink = ink();
    let (w, h) = window_size();
    let mut children = list_pane(&state, ink);
    children.push(detail_pane(&state, ink));
    if let Some((_, sentence)) = description_shown(&state) {
        let tip = tip_rect();
        children.push(part_box(
            TOOLTIP_TAG,
            tip,
            vec![captioned_box(
                "sv.tip.box",
                Rect::new(0, 0, tip.w, tip.h),
                ink.raised,
                Some(ink.edge),
                6,
                (&sentence, FONT_SMALL, ink.text),
                Some(Silence::part_of(TOOLTIP_TAG)),
            )],
        ));
    }
    Scene::Container(
        ContainerNode::new(vec![
            panel(ROOT_TAG, Rect::new(0, 0, w, h), ink.ground, None, children).silenced(
                Silence::layout("places the session list beside the session detail"),
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
    let picked = state.picked();
    let kept = state.kept();
    let mut nodes = vec![
        AccessNode::new(ROOT_TAG, AriaRole::Group)
            .with_name("Sessions")
            .with_child(LIST_TAG)
            .with_child(DETAIL_TAG),
        AccessNode::new(LIST_TAG, AriaRole::Group).with_name("Sessions"),
        AccessNode::new(DETAIL_TAG, AriaRole::Group).with_name("Session detail"),
    ];
    // Every part of both surfaces is named, from the SAME table the pin is
    // compared against — a part the specification gains arrives named.
    //
    // ★★★★★ R1953 — LESS every part that gets a declaration of its own below.
    // `sv.detail.close` was pushed twice, once as a named `Group` here and once
    // as a disabled `Button` at the end, so one address carried two
    // accessibility nodes and a reader taking the first got neither the role
    // nor the refusal. The sibling section had the same defect on five tags.
    // The skip list is DERIVED from the pushes that would collide rather than
    // written out, so a control added later cannot re-open it.
    let mut parts: Vec<AccessNode> = Vec::new();
    for (stem, table) in [("sv.list", spec::LIST), ("sv.detail", spec::DETAIL)] {
        for part in table {
            parts.push(
                AccessNode::new(format!("{stem}.{}", part.key), AriaRole::Group)
                    .with_name(part.title),
            );
        }
    }
    for (n, chip) in spec::CHIPS.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("sv.chip.{}", chip.key), AriaRole::RadioButton)
                .with_name(chip.title)
                .with_selected(state.chip.get() == n)
                .with_set_position(n, spec::CHIPS.len()),
        );
    }
    for (visual, session) in kept.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("sv.row.{}", session.id), AriaRole::Button)
                .with_name(format!(
                    "{} to {}, {}",
                    session.id,
                    session.peer,
                    session.standing.label()
                ))
                .with_selected(session.id == picked.id)
                .with_set_position(visual, kept.len()),
        );
    }
    nodes.push(
        AccessNode::new("sv.detail.close", AriaRole::Button)
            .with_name("Close session")
            .with_state(AccessState {
                disabled: true,
                ..AccessState::default()
            }),
    );
    // ★ R1953 — the table's parts, less every tag something above declared for
    // itself. See the note where `parts` is built.
    let richer: std::collections::BTreeSet<&str> =
        nodes.iter().map(|node| node.tag.as_str()).collect();
    let kept: Vec<AccessNode> = parts
        .iter()
        .filter(|part| !richer.contains(part.tag.as_str()))
        .cloned()
        .collect();
    nodes.extend(kept);
    if let Some((tag, sentence)) = description_shown(state) {
        pinion_widget_paint::described::announce_description(
            &mut nodes,
            &tag,
            TOOLTIP_TAG,
            &sentence,
        );
    }
    let _ = focused;
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
        Hit::Row(id) => Some(format!("sv.row.{id}")),
        Hit::Chip(n) => Some(format!("sv.chip.{}", spec::CHIPS[n].key)),
        Hit::Cross => Some("sv.detail.topology".to_owned()),
        Hit::Close => Some("sv.detail.close".to_owned()),
        Hit::Nothing => None,
    }
}

/// What this section publishes about its own specification.
fn spec_json() -> serde_json::Value {
    let (active, closed) = spec::tally();
    serde_json::json!({
        "at": { "width": WIN_W, "height": WIN_H },
        "sessions": spec::SESSIONS.len(),
        "active": active,
        "closed": closed,
        "columns": spec::COLUMNS.iter().map(|c| c.key).collect::<Vec<_>>(),
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
                    SchemaField::new("session", "json"),
                    SchemaField::new("chip", "string"),
                    SchemaField::new("kept", "int"),
                    SchemaField::new("crossing", "string"),
                    SchemaField::new("said", "string"),
                    SchemaField::parametric(
                        "hit.<x>.<y>",
                        "string",
                        const { &[SchemaArg::open("x", "int"), SchemaArg::open("y", "int")] },
                    ),
                    SchemaField::action("select", "string"),
                    SchemaField::action("chip", "string"),
                    SchemaField::action("press", "string"),
                    SchemaField::action("point", "string"),
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
            return Ok(IntrospectValue::Text(
                Hit::at(state, px, py)
                    .word()
                    .unwrap_or_else(|| "none".to_owned()),
            ));
        }
        match path {
            "spec" => Ok(IntrospectValue::Json(spec_json())),
            "described" => Ok(IntrospectValue::Json(described_wire())),
            "conformance" => Ok(IntrospectValue::Json(
                serde_json::to_value(judge::conformance().to_json())
                    .unwrap_or(serde_json::Value::Null),
            )),
            "selected" => Ok(IntrospectValue::Text(state.selected.get())),
            "session" => {
                let session = state.picked();
                Ok(IntrospectValue::Json(serde_json::json!({
                    "id": session.id,
                    "peer": session.peer,
                    "zid": session.zid,
                    "role": session.role,
                    "link": session.link,
                    "standing": session.standing.label(),
                    "severity": session.standing.severity(),
                    "encryption": session.encryption,
                    "uptime": session.uptime,
                    "rate": session.rate,
                    "version": session.version,
                    "batch": session.batch,
                    "resolution": session.resolution,
                    "handshake": session
                        .timeline()
                        .iter()
                        .map(|step| step.label)
                        .collect::<Vec<_>>(),
                })))
            }
            "chip" => Ok(IntrospectValue::Text(state.chip().key.to_owned())),
            "kept" => Ok(IntrospectValue::Int(
                i64::try_from(state.kept().len()).unwrap_or(0),
            )),
            // ★ The crossing REQUEST, on the wire. A host reads it and moves;
            // a standalone window has nobody listening, and the empty answer
            // says exactly that rather than pretending nothing was asked.
            "crossing" => Ok(IntrospectValue::Text(
                state.crossing.get().unwrap_or_default(),
            )),
            "said" => Ok(IntrospectValue::Text(state.said_sentence())),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    /// ★★★★★ R1953 — the pointer LEAVING, which this section could not hear;
    /// see the twin section's identical arm for what that cost and why it is
    /// on `invoke` rather than `intervene`.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let Some(state) = self.state.clone() else {
            return Err(InvokeError::UnknownPath);
        };
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let IntrospectValue::Text(event) = args else {
            return Err(InvokeError::rejected("a pointer event is a word"));
        };
        match event.trim() {
            "PointerDown" | "PointerUp" | "PointerEnter" => {}
            "PointerLeave" | "PointerCancel" => {
                state.pointer_inside.set(false);
                state.resting.set(None);
            }
            other => {
                return Err(InvokeError::rejected(format!(
                    "{other:?} is not a pointer event; they are PointerDown / \
                     PointerUp / PointerEnter / PointerLeave / PointerCancel"
                )));
            }
        }
        Ok(IntrospectValue::Text(event))
    }

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
                if spec::session(&id).is_none() {
                    return Err(InterveneError::out_of_range(format!(
                        "{id} is not a session"
                    )));
                }
                select_session(&state, &id);
            }
            "chip" => {
                let key = word(&args)?;
                let nth = spec::CHIPS
                    .iter()
                    .position(|c| c.key == key)
                    .ok_or_else(|| InterveneError::out_of_range(format!("{key} is not a chip")))?;
                choose_chip(&state, nth);
            }
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

/// ★ R1948 — public from the first round, because this screen is both a window
/// of its own and a **page** of the analysis-tool shell
/// (`pinion_screen::Mount<SessionsView>`).
pub struct SessionsView;

impl WidgetCore for SessionsView {
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

    /// This screen's marks are addressed under `sv.`, not under its root tag.
    fn paint_stems() -> Vec<&'static str> {
        vec![VIEW_TAG, "sv"]
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, *frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-sessions-view (R1948 §5.41 sessions section)"
    }

    /// ★★★★★ The arrows are this section's only while the focus is this
    /// section's.
    ///
    /// R1947 measured what the other answer costs: a mounted section that
    /// answers every arrow steals the host's rail walk, and it is
    /// UNREPRODUCIBLE in the standalone window, where nothing else is
    /// focusable. Written this way from the first line here because that round
    /// paid for the lesson.
    fn apply_key(
        _scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let mine = focused.is_none_or(|tag| tag == VIEW_TAG || tag.starts_with("sv."));
        mine && key_at(&use_view_state(), chord)
    }
}

impl WidgetA11y for SessionsView {
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        access_nodes(&use_view_state(), focused)
    }

    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        let state = use_view_state();
        (focused == Some(ROWS_TAG))
            .then(|| AccessFocus::composite(ROWS_TAG, format!("sv.row.{}", state.picked().id)))
    }
}

impl WidgetView for SessionsView {
    type Renderer = HelloSessionsViewRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::shrinking(SHRINK, (WIN_W, WIN_H))
    }

    fn shrink_policy() -> Option<ShrinkPolicy> {
        Some(SHRINK)
    }

    fn conformance() -> Option<pinion_core::conformance::DocumentReport> {
        Some(judge::conformance())
    }
}

/// ★★★★★ R1948 — **the peer this section last asked to be shown in the
/// topology section, if any.**
///
/// The host reads this and moves; nobody else does. It is a REQUEST rather than
/// a navigation because a page that moved the rail would be a page deciding
/// what application it is in — the same reason every other cross-screen fact in
/// this tree is published rather than performed.
#[must_use]
pub fn crossing_request() -> Option<String> {
    use_view_state().crossing.get()
}

/// Clear the crossing request, once a host has acted on it.
#[must_use]
pub fn take_crossing() -> Option<String> {
    let state = use_view_state();
    let asked = state.crossing.get();
    state.crossing.set(None);
    asked
}

/// Run the sessions section as an application of its own.
pub fn run() {
    pinion_shell::run::<SessionsView>();
}
