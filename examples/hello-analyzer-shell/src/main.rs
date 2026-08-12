// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-analyzer-shell` — R1648/R1649 §5.21 §5.51 §2 #7 — the analysis-tool
//! **dashboard shell**, assembled as one application and operated by hand.
//!
//! ## Why an assembly is the deliverable
//!
//! `tools/analyzer_census.py` classifies every capability of this tool class
//! with one of five verdicts, and the largest bin is `app`: *the substrate is
//! here, the domain logic is the application's*. A `have` verdict is proven by
//! a test that exercises the capability through the public API (R1602); an
//! `app` verdict is a claim about composition, and the only thing that proves
//! composition is a composite. This is that composite, and the census names it.
//!
//! ## R1649 — the shell matches the reference tool's, screen and gesture
//!
//! R1648 assembled the *capabilities* and laid them out in the plainest
//! arrangement that could hold them. R1649 rebuilds the chrome and the
//! placement gestures against the tool this axis is judged by, because a shell
//! is exactly the part where "the substrate can express it" is not the same
//! claim as "the substrate can express it the way a professional tool does".
//! The differences that cost real work were all the second claim:
//!
//! * **A three-column shell**, not a board with a strip: an icon rail, a
//!   sub-header carrying the layout preset and the two board verbs, the canvas,
//!   and a **widget palette** the board is populated *from*. Twelve widget
//!   kinds exist as a catalogue; three are placed.
//! * **A card is added, not seeded.** The palette's `+` places one at the
//!   bottom of the board, so what is on the board is a decision somebody made
//!   and the count in two places has to agree.
//! * **A drag shows where it will land** — a snap-preview slot and a brighter
//!   grid — rather than displacing cards live. The reference commits on
//!   release; so does this.
//! * **Layout-edit mode** puts per-card size steppers on the cards, so a board
//!   is rearranged without a pointer drag at all.
//! * **Detaching REMOVES the card from the board** and opens a floating panel
//!   with a re-dock control; re-docking appends it at the bottom. R1648 kept
//!   the tile in place and argued for it — the reference does not, and on this
//!   axis the reference is the specification rather than a data point.
//!
//! ## The state vocabulary is a value, not paint
//!
//! Every card carries a [`Card`]: its header affordances as a set, and what its
//! body is showing as a [`CardState`] whose [`Remedy`] is **derived**. The pair
//! that justifies six arms rather than one `Error` is denied/opaque — both
//! render as "no content" and they are opposite in what a person can do, so a
//! shell with one arm offers "request access" on a link no permission can open.
//! Measured on the toolkit at 6.11: no content-state concept exists on any
//! panel or view class, and its item views have no placeholder at all.
//!
//! ## It is operated by hand, and by the wire, through one set of handlers
//!
//! ```text
//! cargo run -p hello-analyzer-shell --release
//! ```
//!
//! Add a widget from the palette, drag a card by its header, press the header
//! controls, toggle layout-edit mode and use the size steppers, detach a card
//! and re-dock it, switch a saved layout. The keyboard is [`KEYMAP`].
//!
//! A real press and a scripted one reach the same code: the framework's router
//! calls `pointer_move` and sends `PointerDown` / `PointerUp` through
//! `invoke("send", …)`, and the wire's `point` moves the same cursor. There is
//! no parallel automation surface that can drift from what a hand does.
//!
//! The geometry is likewise one thing — every rectangle comes from the helpers
//! above [`Hit::at`], read by both the painter and the hit test. The demo
//! sweeps the window in both directions to keep it so, and the second direction
//! (every painted control must be pressable at the centre of the rectangle it
//! was painted in) is what caught R1648 painting every card's contents at twice
//! their intended offset: children of an absolutely-positioned container are
//! placed relative to *it*.
//!
//! See `tools/demos/r1648_the_analyzer_shell_is_assembled.py`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, Sparkline};
use pinion_core::availability::Unavailable;
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    Border, BoxStyle, Color, LayoutStyle, PathStyle, Size, Stroke, TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, ThemeMode, ThemeProvider, use_theme};
use pinion_core::widgets::card::{Card, CardAffordance, CardChrome, CardState, Remedy};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::tile_grid::{
    Maximized, Tile, TileDirection, TileGrid, TileId, TileNudge,
};
use pinion_core::widgets::transport::{TransportClock, TransportStatus, use_transport_clock};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::pane::{PanePointer, scroll_pane};

mod spec;

// pinion-forge codegen output: `pub struct HelloAnalyzerShellRenderer` + its
// error type + async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloAnalyzerShellRenderer, HelloAnalyzerShellRendererError);

const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;

const VIEW_TAG: &str = "analyzer_shell";
const THEME_TAG: &str = "app";
const STATE_KEY: &str = "hello-analyzer-shell/state";
const TRANSPORT_KEY: &str = "hello-analyzer-shell/transport";

/// The replay window a scrub moves through, in seconds.
const REPLAY_SECS: f32 = 12.0;

// --- Shell metrics -----------------------------------------------------------
//
// The three the reference tool states as constants — twelve columns, a fixed
// row height and a gutter — are the same here, because a board's arrangement is
// only portable between two tools that agree on what a cell is.

const APP_BAR_H: u32 = spec::APP_BAR_H;
const SUB_BAR_H: u32 = spec::SUB_BAR_H;
const RAIL_W: u32 = spec::RAIL_W;
const PALETTE_W: u32 = spec::PALETTE_W;
const GRID_COLS: u32 = spec::GRID_COLS;
const ROW_H: u32 = 174;
const GAP: u32 = 16;
/// A card's header strip.
const CARD_HDR: u32 = 34;
/// (R1668) The decode card's byte pane: the width it prefers, and the narrowest
/// it can be and still show an offset plus one byte. Below the floor the pane
/// is dropped rather than drawn outside the card.
const BYTES_W: u32 = 148;
const BYTES_FLOOR: u32 = 66;
/// (R1668) A decode row's value column, and the narrowest its key can be. The
/// key is allocated first: a row that lost its name reads as a value belonging
/// to nothing.
const VALUE_W: u32 = 74;
const KEY_FLOOR: u32 = 30;
/// (R1668) A filter stat tile: its height, and the narrowest it can be and
/// still hold a number. The three go or stay together.
const STAT_H: u32 = 46;
const STAT_FLOOR: u32 = 52;
/// The size-stepper strip layout-edit mode adds to the foot of every card.
const EDIT_BAR_H: u32 = 26;
/// A detached panel's opening size, and the cascade between successive ones.
const FLOAT_W: u32 = 520;
const FLOAT_H: u32 = 380;
const FLOAT_STEP: u32 = 30;

/// R1662 — the input-router tag the board's scrolling body answers to.
const CANVAS_SCROLL: &str = "shell.canvas.body";

const FONT_TITLE: u32 = 13;
const FONT_BODY: u32 = 12;
const FONT_SMALL: u32 = 11;
const FONT_TINY: u32 = 10;

/// The canvas rectangle: everything between the rail and the palette, under
/// both bars.
const fn canvas_rect() -> Rect {
    Rect::new(
        RAIL_W,
        APP_BAR_H + SUB_BAR_H,
        WIN_W - RAIL_W - PALETTE_W,
        WIN_H - APP_BAR_H - SUB_BAR_H,
    )
}

/// One grid column's pitch, gutters included.
fn col_pitch() -> u32 {
    (canvas_rect().w.saturating_sub(GAP)) / GRID_COLS
}

// --- The widget catalogue ----------------------------------------------------
//
// The table itself is in `spec.rs` — the reference's own catalogue written down
// as a value, thirteen entries of which four the first release places and nine
// it reserves. What stays here is what is about PAINT rather than about the
// screen: the colour a kind is identified by, and the header controls a card
// carries. Everything else is read from the specification, so this file cannot
// drift from it without failing the gate in `painted.rs`.

use CardAffordance::{Close, Maximize, Settings, TearOff};

/// Every placed card carries the same header controls.
///
/// The specification names them and this maps the names onto the framework's
/// vocabulary, so a control added there arrives here rather than being
/// remembered: a uniform board is what makes a missing control legible, where a
/// board of individually-decided chrome is a board nobody can check.
fn chrome() -> Vec<CardAffordance> {
    spec::CARD_CHROME
        .iter()
        .map(|name| match *name {
            "settings" => Settings,
            "tear_off" => TearOff,
            "maximize" => Maximize,
            "close" => Close,
            other => panic!("the specification names a header control {other:?} this shell has no affordance for"),
        })
        .collect()
}

/// A colour from a packed `0xRRGGBB`, so the reference's token table can be
/// transcribed exactly as it is written rather than as three decimals each.
#[allow(
    clippy::cast_possible_truncation,
    reason = "each mask isolates one byte, which is what the cast takes"
)]
const fn rgb(hex: u32) -> Color {
    Color::rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// The colour a kind is identified by — on its palette swatch and on its card.
///
/// Keyed by the specification's kind rather than stored beside it: a colour is
/// a decision about ink and the specification is a statement about the screen,
/// and this round's whole point is that the two are not the same document.
fn kind_color(kind: &str) -> Color {
    match kind {
        "packet" => rgb(0x2D_6C_DF),
        "decode" => rgb(0x8A_5C_F6),
        "keymap" => rgb(0xC7_78_00),
        "filter" => rgb(0x1F_8A_4C),
        "topology" => rgb(0x9A_00_4F),
        "overlay" => rgb(0xB0_33_5B),
        "throughput" => rgb(0x0E_9A_A7),
        "share" => rgb(0x35_C0_8B),
        "latency" => rgb(0xD3_3A_2C),
        "health" => rgb(0x4A_7A_9B),
        "loss" => rgb(0xA9_4F_10),
        "alarms" => rgb(0x7A_4B_C4),
        "admin" => rgb(0x2F_6B_6B),
        _ => rgb(0x6B_72_80),
    }
}

/// The cells a kind occupies when it is placed, from the opening board.
///
/// A reserved kind has no size because it is never placed, which is why this
/// answers `None` for one rather than inventing a default that no screen has
/// ever painted.
fn kind_span(kind: &str) -> Option<(u32, u32)> {
    spec::BOARD
        .iter()
        .find(|p| p.kind == kind)
        .map(|p| (p.cols, p.rows))
}

/// The catalogue entry for a kind.
fn def_of(kind: &str) -> Option<&'static spec::WidgetSpec> {
    spec::widget_of(kind)
}

/// The sources the application bar offers. The first is the one the
/// specification states the screen opens on; the others are what the control
/// cycles to, which the specification does not fix.
///
/// No real address belongs in a repository, so these are documentation
/// addresses (RFC 5737 TEST-NET-1).
const SOURCES: [&str; 3] = [
    spec::SOURCE,
    "lo \u{00B7} 127.0.0.1:7447",
    "file \u{00B7} session-2.capture",
];

/// The two view tabs the application bar carries.
const TABS: [&str; 2] = ["Dashboard", "Design System"];

/// A card's id is `<kind>#<n>` — the kind so the definition is recoverable
/// without a side table, the ordinal so a kind can be placed more than once.
fn kind_of(id: &str) -> &str {
    id.split_once('#').map_or(id, |(kind, _)| kind)
}

fn def_for_card(id: &str) -> Option<&'static spec::WidgetSpec> {
    def_of(kind_of(id))
}

fn label_of(id: &str) -> String {
    def_for_card(id).map_or_else(|| id.to_string(), |d| d.label.to_string())
}

/// The sparkline the filter card draws under its counts: how many messages
/// matched, over the recent past.
const MATCH_SERIES: [f64; 12] = [
    4.0, 6.0, 5.0, 9.0, 7.0, 12.0, 10.0, 14.0, 11.0, 15.0, 13.0, 17.0,
];

/// The reference tool's own dark and light token sets, mapped onto this
/// framework's roles.
///
/// A tool of this class is looked at for hours in a dim room, so its palette is
/// a decision rather than a default, and matching it is most of what makes two
/// screens look like the same product. The mapping is one-way and explicit:
/// the canvas is the darkest ground, panels sit on it, the raised chrome is one
/// step lighter again, and one accent serves every affirmative control.
fn reference_palettes() -> (Theme, Theme) {
    let dark = Theme {
        surface: rgb(0x0E_0F_12),
        on_surface: rgb(0xE8_EB_EF),
        on_surface_muted: rgb(0x98_A2_AD),
        accent: rgb(0x9A_00_4F),
        on_accent: rgb(0xFF_FF_FF),
        outline: rgb(0x2A_2E_36),
        surface_container_low: rgb(0x16_18_1D),
        surface_container: rgb(0x1E_21_27),
        surface_container_high: rgb(0x25_2A_33),
        surface_container_highest: rgb(0x3A_40_4B),
        error: rgb(0xF0_70_5E),
        inverse_primary: rgb(0xEC_5A_A0),
        ..Theme::dark()
    };
    let light = Theme {
        surface: rgb(0xF6_F7_F9),
        on_surface: rgb(0x14_17_1C),
        on_surface_muted: rgb(0x5B_65_70),
        accent: rgb(0x9A_00_4F),
        on_accent: rgb(0xFF_FF_FF),
        outline: rgb(0xE1_E5_EA),
        surface_container_low: rgb(0xFF_FF_FF),
        surface_container: rgb(0xEE_F0_F3),
        surface_container_high: rgb(0xE7_EA_EF),
        surface_container_highest: rgb(0xCD_D3_DB),
        error: rgb(0xD3_3A_2C),
        inverse_primary: rgb(0x9A_00_4F),
        ..Theme::light()
    };
    (light, dark)
}

/// The canvas's dot-grid colour, which is not a theme role: it is a hairline
/// that must read as *below* the surface rather than on it.
fn grid_ink(dark: bool) -> Color {
    if dark {
        rgb(0x20_24_2C)
    } else {
        rgb(0xE4_E8_ED)
    }
}

// --- State -------------------------------------------------------------------

/// A detached panel: a card that has left the board and floats over it.
///
/// Serialisable because it lives in a `Signal`, and because a session saved
/// with a panel torn off must reopen with it torn off — the same reason the
/// arrangement is serialisable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Float {
    id: String,
    x: u32,
    y: u32,
}

/// A drag in flight: which card, where inside it the grab landed, and the cell
/// a release would put it in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Drag {
    id: TileId,
    dx: u32,
    dy: u32,
    snap: (u32, u32),
}

/// A saved layout: the arrangement AND which cards were on it.
///
/// Both, because a board is two facts — where the cells are and what is in
/// them — and a preset restoring only the first would put the previous board's
/// cards into the new layout's holes.
#[derive(Debug, Clone)]
struct Preset {
    board: TileGrid,
    cards: Vec<Card>,
}

struct ShellState {
    clock: Rc<TransportClock>,
    theme: Rc<ThemeProvider>,
    board: Signal<TileGrid>,
    cards: Signal<Vec<Card>>,
    /// `Some` exactly while a card is maximised. The token IS the way home.
    maximized: Signal<Option<Maximized>>,
    floats: Signal<Vec<Float>>,
    presets: RefCell<BTreeMap<String, Preset>>,
    preset: Signal<String>,
    preset_open: Signal<bool>,
    /// Layout-edit mode: the size steppers appear and the grid brightens.
    editing: Signal<bool>,
    /// Which card's settings strip is open, if any.
    config_open: Signal<Option<String>>,
    source: Signal<String>,
    capturing: Signal<bool>,
    search: Signal<String>,
    searching: Signal<bool>,
    tab: Signal<String>,
    nav: Signal<String>,
    selected: Signal<Option<String>>,
    cursor: Signal<(u32, u32)>,
    pressed: RefCell<Option<Hit>>,
    drag: Signal<Option<Drag>>,
    /// The last thing that happened, shown as the reference's toast.
    toast: Signal<String>,
    /// The ordinal the next placed card takes.
    next_id: RefCell<u32>,
    /// R1662 — the board's scroll offset. A board is a grid whose row count is
    /// the model's, not the window's, so past roughly four and a half rows the
    /// cards were painted below the window and no gesture reached them
    /// ([[debt-the-analyzer-canvas-does-not-scroll]]). Held on the state
    /// because the paint and the hit test both read it.
    canvas_scroll: Rc<ScrollState>,
}

impl ShellState {
    fn new(clock: Rc<TransportClock>, theme: Rc<ThemeProvider>) -> Self {
        let (light, dark) = reference_palettes();
        theme.set_palettes(light, dark);
        theme.set_mode(ThemeMode::Dark);
        let mut board = TileGrid::new(GRID_COLS);
        let mut cards = Vec::new();
        for (n, placed) in spec::BOARD.iter().enumerate() {
            let def = def_of(placed.kind).expect("the board names catalogue kinds");
            let id = format!("{}#{n}", placed.kind);
            board
                .place(Tile::new(
                    id.clone(),
                    placed.col,
                    placed.row,
                    placed.cols,
                    placed.rows,
                ))
                .expect("the specified board is a legal arrangement");
            cards.push(
                Card::new(id, def.label)
                    .with_chrome(CardChrome::of(chrome()))
                    .with_state(CardState::Ready),
            );
        }
        let mut presets = BTreeMap::new();
        presets.insert(
            spec::PRESET.to_string(),
            Preset {
                board: board.clone(),
                cards: cards.clone(),
            },
        );
        Self {
            clock,
            theme,
            board: Signal::new(board),
            cards: Signal::new(cards),
            maximized: Signal::new(None),
            floats: Signal::new(Vec::new()),
            presets: RefCell::new(presets),
            preset: Signal::new(spec::PRESET.to_string()),
            preset_open: Signal::new(false),
            editing: Signal::new(false),
            config_open: Signal::new(None),
            source: Signal::new(SOURCES[0].to_string()),
            capturing: Signal::new(true),
            search: Signal::new(String::new()),
            searching: Signal::new(false),
            tab: Signal::new(TABS[0].to_string()),
            nav: Signal::new(spec::RAIL_ACTIVE.to_string()),
            selected: Signal::new(None),
            cursor: Signal::new((0, 0)),
            pressed: RefCell::new(None),
            drag: Signal::new(None),
            toast: Signal::new(format!("{} loaded", spec::PRESET)),
            next_id: RefCell::new(u(spec::BOARD.len())),
            canvas_scroll: Rc::new(ScrollState::with_tag(CANVAS_SCROLL)),
        }
    }

    fn card(&self, id: &str) -> Option<Card> {
        self.cards.get().into_iter().find(|c| c.id().as_str() == id)
    }

    fn update_card(&self, id: &str, edit: impl FnOnce(&mut Card)) -> bool {
        let mut cards = self.cards.get();
        let Some(found) = cards.iter_mut().find(|c| c.id().as_str() == id) else {
            return false;
        };
        edit(found);
        self.cards.set(cards);
        true
    }

    fn card_ids(&self) -> String {
        self.cards
            .get()
            .iter()
            .map(|c| c.id().as_str().to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The cards actually on the board — a detached one is not.
    fn placed(&self) -> Vec<Card> {
        let floating = self.floats.get();
        self.cards
            .get()
            .into_iter()
            .filter(|c| !floating.iter().any(|f| f.id == c.id().as_str()))
            .collect()
    }

    fn is_floating(&self, id: &str) -> bool {
        self.floats.get().iter().any(|f| f.id == id)
    }

    fn preset_names(&self) -> String {
        self.presets
            .borrow()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(",")
    }

    fn say(&self, what: impl Into<String>) {
        self.toast.set(what.into());
    }
}

fn use_shell_state() -> Rc<ShellState> {
    let clock = use_transport_clock(TRANSPORT_KEY, REPLAY_SECS);
    let theme = use_theme(THEME_TAG);
    Owner::current()
        .expect("use_shell_state requires an active Owner scope")
        .cache(STATE_KEY, move || ShellState::new(clock, theme))
}

// --- Geometry: ONE source, read by the paint and by the gesture --------------
//
// debt-paint-and-gesture-read-two-facts is open in this project because a
// surface whose painter and hit test compute their rectangles separately drifts
// into a control drawn where it cannot be clicked. Every rectangle below is
// computed once and used by BOTH the `*_scene` painters and `Hit::at`.

/// A container's own coordinate space: its size at the origin.
///
/// ★ Children of an absolutely-positioned container are placed RELATIVE TO IT,
/// so a child written in window coordinates lands at the parent's origin plus
/// its own. R1648 shipped exactly that bug and only a two-direction sweep of
/// the painted rectangles found it. The sub-rectangles below are **local**; the
/// painter passes `local(rect)` and the hit test subtracts the origin.
const fn local(rect: Rect) -> Rect {
    Rect::new(0, 0, rect.w, rect.h)
}

const fn contains(rect: Rect, px: u32, py: u32) -> bool {
    px >= rect.x && px < rect.x + rect.w && py >= rect.y && py < rect.y + rect.h
}

fn u(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Where a tile sits on the canvas, in the canvas's own space.
fn cell_rect(tile: &Tile) -> Rect {
    let pitch = col_pitch();
    Rect::new(
        GAP + tile.col * pitch,
        GAP + tile.row * ROW_H,
        (tile.w * pitch).saturating_sub(GAP).max(1),
        (tile.h * ROW_H).saturating_sub(GAP).max(1),
    )
}

/// The board cell a canvas-local pixel lands on — the inverse of [`cell_rect`],
/// and the only place the two directions meet.
fn cell_at(lx: u32, ly: u32) -> (u32, u32) {
    let pitch = col_pitch().max(1);
    let col = lx.saturating_sub(GAP) / pitch;
    let row = ly.saturating_sub(GAP) / ROW_H;
    (col.min(GRID_COLS - 1), row)
}

const fn header_rect(card: Rect) -> Rect {
    Rect::new(card.x, card.y, card.w, CARD_HDR)
}

const fn body_rect(card: Rect, editing: bool) -> Rect {
    let foot = if editing { EDIT_BAR_H } else { 0 };
    Rect::new(
        card.x,
        card.y + CARD_HDR,
        card.w,
        card.h.saturating_sub(CARD_HDR + foot),
    )
}

/// The size-stepper strip at the foot of a card in layout-edit mode.
const fn edit_bar_rect(card: Rect) -> Rect {
    Rect::new(
        card.x,
        (card.y + card.h).saturating_sub(EDIT_BAR_H),
        card.w,
        EDIT_BAR_H,
    )
}

/// One header control slot. Right-aligned, in declaration order, so the
/// rightmost is the last affordance the vocabulary declares.
const SLOT_W: u32 = 28;

const fn affordance_rect(header: Rect, count: u32, n: u32) -> Rect {
    let from_right = count.saturating_sub(n);
    Rect::new(
        (header.x + header.w).saturating_sub(from_right * SLOT_W + 6),
        header.y + 4,
        SLOT_W,
        CARD_HDR - 8,
    )
}

/// The drag handle at the left of a header — the reference's six-dot grip.
const fn grip_rect(header: Rect) -> Rect {
    Rect::new(header.x + 4, header.y + 4, 18, CARD_HDR - 8)
}

/// Where a not-ready card's remedy control sits.
const fn remedy_rect(body: Rect) -> Rect {
    Rect::new(body.x + 10, body.y + 32, 150, 22)
}

/// The size steppers, left to right: `− W + − H +`.
const STEPPERS: [(&str, &str); 4] = [
    ("narrow", "\u{2212}"),
    ("widen", "+"),
    ("shorter", "\u{2212}"),
    ("taller", "+"),
];

/// Where the `n`th stepper button sits in a card's edit bar: two groups of two,
/// with the axis letter between them.
const fn stepper_rect(bar: Rect, n: u32) -> Rect {
    let group = n / 2;
    let within = n % 2;
    Rect::new(
        bar.x + 8 + group * 78 + within * 46,
        bar.y + 3,
        20,
        EDIT_BAR_H - 6,
    )
}

/// The app bar's pressable regions, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarChip {
    Tab0,
    Tab1,
    Source,
    Capture,
    Search,
}

impl BarChip {
    const ALL: [Self; 5] = [
        Self::Tab0,
        Self::Tab1,
        Self::Source,
        Self::Capture,
        Self::Search,
    ];

    const fn tag(self) -> &'static str {
        match self {
            Self::Tab0 => "shell.appbar.tab.dashboard",
            Self::Tab1 => "shell.appbar.tab.design",
            Self::Source => "shell.appbar.source",
            Self::Capture => "shell.appbar.capture",
            Self::Search => "shell.appbar.search",
        }
    }

    const fn rect(self) -> Rect {
        match self {
            Self::Tab0 => Rect::new(168, 10, 108, 32),
            Self::Tab1 => Rect::new(280, 10, 118, 32),
            Self::Source => Rect::new(416, 10, 268, 32),
            Self::Capture => Rect::new(696, 10, 132, 32),
            Self::Search => Rect::new(WIN_W - 300, 10, 288, 32),
        }
    }
}

/// The sub bar's pressable regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubChip {
    Preset,
    EditLayout,
    AddWidget,
}

impl SubChip {
    const ALL: [Self; 3] = [Self::Preset, Self::EditLayout, Self::AddWidget];

    const fn tag(self) -> &'static str {
        match self {
            Self::Preset => "shell.subbar.preset",
            Self::EditLayout => "shell.subbar.edit",
            Self::AddWidget => "shell.subbar.add",
        }
    }

    /// In the sub bar's own space, whose origin is `(RAIL_W, APP_BAR_H)`.
    const fn rect(self) -> Rect {
        let bar_w = WIN_W - RAIL_W - PALETTE_W;
        match self {
            Self::Preset => Rect::new(16, 7, 178, 32),
            Self::EditLayout => Rect::new(bar_w - 330, 7, 140, 32),
            Self::AddWidget => Rect::new(bar_w - 180, 7, 164, 32),
        }
    }
}

/// One entry of the open preset menu, in the sub bar's own space.
const fn preset_item_rect(n: u32) -> Rect {
    let anchor = SubChip::Preset.rect();
    Rect::new(anchor.x + 8, anchor.y + 44 + n * 34, 210, 30)
}

/// Where the `n`th rail entry sits, in the rail container's own space.
const fn rail_rect(n: u32) -> Rect {
    Rect::new(8, 14 + n * 44, RAIL_W - 16, 36)
}

/// One palette entry's height.
///
/// Sized so that the whole catalogue FITS the panel: the reference scrolls its
/// palette and this shell does not, so a row height that overflowed would put
/// the last widget kinds under the footer where nothing can reach them. That is
/// a real difference from the reference and it is spent here rather than
/// hidden — see the module docs' list of what is not matched.
const PALETTE_ROW_H: u32 = 46;

/// The palette panel's rectangle.
const fn palette_rect() -> Rect {
    Rect::new(WIN_W - PALETTE_W, APP_BAR_H, PALETTE_W, WIN_H - APP_BAR_H)
}

/// The palette's rows — section headers interleaved with entries, in the
/// panel's own space.
///
/// Returned rather than recomputed at each site: the painter walks it to draw
/// and the hit test walks it to resolve, which is the discipline the card
/// rectangles follow.
fn palette_rows() -> Vec<(Option<&'static spec::WidgetSpec>, &'static str, Rect)> {
    let mut out = Vec::new();
    let mut y = 76_u32;
    for (key, title, _tier) in spec::SECTIONS {
        out.push((None, *title, Rect::new(16, y, PALETTE_W - 32, 20)));
        y += 26;
        for def in spec::CATALOGUE.iter().filter(|w| w.section == *key) {
            out.push((
                Some(def),
                def.label,
                Rect::new(10, y, PALETTE_W - 30, PALETTE_ROW_H),
            ));
            y += PALETTE_ROW_H + 4;
        }
        y += 8;
    }
    out
}

// --- What is under a point ---------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    Chip(BarChip),
    Sub(SubChip),
    PresetItem(usize),
    Rail(&'static str),
    Palette(&'static str),
    Grip(String),
    Affordance(String, CardAffordance),
    Stepper(String, &'static str),
    Remedy(String),
    Card(String),
    FloatRedock(String),
    FloatClose(String),
    Float(String),
    Nothing,
}

impl Hit {
    /// Front to back: the preset menu is over the sub bar, floats are over the
    /// canvas, and a card's own controls are over its body.
    fn at(state: &ShellState, px: u32, py: u32) -> Self {
        let sub_origin = (RAIL_W, APP_BAR_H);
        if state.preset_open.get() && px >= sub_origin.0 && py >= sub_origin.1 {
            let (lx, ly) = (px - sub_origin.0, py - sub_origin.1);
            let rows = state.presets.borrow().len();
            for n in 0..=rows {
                if contains(preset_item_rect(u(n)), lx, ly) {
                    return Self::PresetItem(n);
                }
            }
        }
        if py < APP_BAR_H {
            for chip in BarChip::ALL {
                if contains(chip.rect(), px, py) {
                    return Self::Chip(chip);
                }
            }
            return Self::Nothing;
        }
        if px >= palette_rect().x {
            let panel = palette_rect();
            let (lx, ly) = (px - panel.x, py - panel.y);
            for (def, _title, rect) in palette_rows() {
                if let Some(def) = def
                    && contains(rect, lx, ly)
                {
                    return Self::Palette(def.kind);
                }
            }
            return Self::Nothing;
        }
        if px < RAIL_W {
            for (n, seat) in spec::RAIL.iter().enumerate() {
                if contains(rail_rect(u(n)), px, py - APP_BAR_H) {
                    return Self::Rail(seat.key);
                }
            }
            return Self::Nothing;
        }
        if py < APP_BAR_H + SUB_BAR_H {
            let (lx, ly) = (px - sub_origin.0, py - sub_origin.1);
            for chip in SubChip::ALL {
                if contains(chip.rect(), lx, ly) {
                    return Self::Sub(chip);
                }
            }
            return Self::Nothing;
        }
        let canvas = canvas_rect();
        Self::in_canvas(state, px - canvas.x, py - canvas.y)
    }

    /// What is under a point in the canvas's own space.
    ///
    /// Split out of [`Self::at`] because the canvas is the one region with a
    /// stacking order of its own — floats over cards, a card's controls over
    /// its body — and reading that order should not mean scrolling past the
    /// four chrome regions first.
    fn in_canvas(state: &ShellState, cx: u32, cy: u32) -> Self {
        // Floats are over the canvas, newest first.
        for float in state.floats.get().iter().rev() {
            let rect = float_rect(float);
            if !contains(rect, cx, cy) {
                continue;
            }
            let (lx, ly) = (cx - rect.x, cy - rect.y);
            let header = header_rect(local(rect));
            if contains(header, lx, ly) {
                if contains(affordance_rect(header, 2, 1), lx, ly) {
                    return Self::FloatClose(float.id.clone());
                }
                if contains(affordance_rect(header, 2, 0), lx, ly) {
                    return Self::FloatRedock(float.id.clone());
                }
            }
            return Self::Float(float.id.clone());
        }
        // ★ R1662 — past the floats the question is about the BOARD, which
        // slides under the canvas. Every rectangle below is stated in the
        // board's own frame, so the offset is folded into the query once and
        // the two cannot drift.
        let (ox, oy) = state.canvas_scroll.offset();
        let fold = |v: u32, by: i32| -> u32 {
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "clamped into u32's range on the line above the cast"
            )]
            let folded = (i64::from(v) + i64::from(by)).clamp(0, i64::from(u32::MAX)) as u32;
            folded
        };
        let (cx, cy) = (fold(cx, ox), fold(cy, oy));
        let board = state.board.get();
        let editing = state.editing.get();
        for card in &state.placed() {
            let Some(tile) = board.tile(card.id()) else {
                continue;
            };
            let rect = cell_rect(tile);
            if !contains(rect, cx, cy) {
                continue;
            }
            let (lx, ly) = (cx - rect.x, cy - rect.y);
            let inside = local(rect);
            let id = card.id().as_str().to_string();
            let header = header_rect(inside);
            if contains(header, lx, ly) {
                let offered = card.chrome().offered();
                for (n, affordance) in offered.iter().enumerate() {
                    if contains(affordance_rect(header, u(offered.len()), u(n)), lx, ly) {
                        return Self::Affordance(id, *affordance);
                    }
                }
                // The whole header drags, as it does in the reference; the grip
                // is where it SAYS so.
                return Self::Grip(id);
            }
            if editing {
                let bar = edit_bar_rect(inside);
                if contains(bar, lx, ly) {
                    for (n, (verb, _)) in STEPPERS.iter().enumerate() {
                        if contains(stepper_rect(bar, u(n)), lx, ly) {
                            return Self::Stepper(id, verb);
                        }
                    }
                    return Self::Card(id);
                }
            }
            if card.remedy().is_some_and(Remedy::is_actionable)
                && contains(remedy_rect(body_rect(inside, editing)), lx, ly)
            {
                return Self::Remedy(id);
            }
            return Self::Card(id);
        }
        Self::Nothing
    }

    fn card_id(&self) -> Option<&str> {
        match self {
            Self::Affordance(id, _)
            | Self::Remedy(id)
            | Self::Card(id)
            | Self::Grip(id)
            | Self::Stepper(id, _) => Some(id),
            _ => None,
        }
    }
}

/// A detached panel's rectangle, in the canvas's own space.
const fn float_rect(float: &Float) -> Rect {
    Rect::new(float.x, float.y, FLOAT_W, FLOAT_H)
}

/// One hit, named by the **scene tag** of the thing that was hit.
///
/// R1614's lesson — a name that has to survive is an address, not a
/// description — and the demo enforces it by sweeping the window and requiring
/// every name this returns to be a tag the paint actually emitted.
fn hit_word(hit: &Hit) -> String {
    match hit {
        Hit::Chip(chip) => chip.tag().to_string(),
        Hit::Sub(chip) => chip.tag().to_string(),
        Hit::PresetItem(n) => format!("shell.preset.item.{n}"),
        Hit::Rail(name) => format!("shell.rail.{name}"),
        Hit::Palette(kind) => format!("shell.palette.{kind}"),
        Hit::Grip(id) => format!("card.{id}.grip"),
        Hit::Affordance(id, affordance) => format!("card.{id}.{}", affordance.wire()),
        Hit::Stepper(id, verb) => format!("card.{id}.{verb}"),
        Hit::Remedy(id) => format!("card.{id}.remedy"),
        Hit::Card(id) => format!("card.{id}"),
        Hit::FloatRedock(id) => format!("float.{id}.redock"),
        Hit::FloatClose(id) => format!("float.{id}.close"),
        Hit::Float(id) => format!("float.{id}"),
        Hit::Nothing => "nothing".to_string(),
    }
}

/// The chords this shell claims, published as well as painted.
const KEYMAP: [(&str, &str); 12] = [
    ("/", "type into the global search; Enter or Escape leaves"),
    ("Arrow", "move the selection to the neighbouring card"),
    ("Shift+Arrow", "move the selected card one cell"),
    ("Alt+Arrow", "grow that side of the card"),
    ("Alt+Shift+Arrow", "shrink that side"),
    ("Enter", "maximise the selection, or restore"),
    ("Escape", "restore a maximised board"),
    ("Delete", "close the selected card"),
    ("e", "toggle layout edit mode"),
    ("o", "detach the selected card, or re-dock it"),
    ("r", "act on the selected card's remedy"),
    ("c / t / s", "capture / theme / source"),
];

const HELP_STRIP: &str = "drag a header to move \u{00B7} e edit \u{00B7} o detach \u{00B7} Enter max \u{00B7} \
     Esc restore \u{00B7} Del close \u{00B7} / search";

// --- The oracle (primary External) ------------------------------------------

struct ShellOracle {
    state: Option<Rc<ShellState>>,
    /// R1656 §5.15 — the size the shell says this surface currently has.
    ///
    /// Kept because `External::pointer_move` hands a FRACTION of it and not the
    /// rectangle itself, so a consumer that wants pixels has to hold the basis.
    /// Seeded with the opening size and replaced by every
    /// [`External::on_resize`].
    surface: (u32, u32),
}

impl core::fmt::Debug for ShellOracle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ShellOracle")
            .field("attached", &self.state.is_some())
            .field("surface", &self.surface)
            .finish()
    }
}

impl ShellOracle {
    const NO_STATE: &str = "this shell surface is not bound to a model yet";

    const fn new() -> Self {
        Self {
            state: None,
            surface: (WIN_W, WIN_H),
        }
    }

    fn attach_state(&mut self, state: Rc<ShellState>) {
        self.state = Some(state);
    }

    fn state(&self) -> Result<&Rc<ShellState>, InvokeError> {
        self.state
            .as_ref()
            .ok_or_else(|| InvokeError::rejected(Self::NO_STATE))
    }

    fn text(arg: &IntrospectValue) -> Result<String, InvokeError> {
        match arg {
            IntrospectValue::Text(s) => Ok(s.clone()),
            other => Err(InvokeError::rejected(format!(
                "expected a string argument, got {other:?}"
            ))),
        }
    }

    fn card_of(state: &ShellState, id: &str) -> Result<Card, InvokeError> {
        state.card(id).ok_or_else(|| {
            InvokeError::rejected(format!(
                "no card {id:?} on this board; it holds {}",
                state.card_ids()
            ))
        })
    }

    /// The reads that take a card id, answered together.
    fn card_read(
        state: &Rc<ShellState>,
        path: &str,
        args: &IntrospectValue,
    ) -> Option<Result<IntrospectValue, InvokeError>> {
        let wanted = matches!(
            path,
            "title" | "chrome" | "section" | "state" | "detail" | "remedy" | "actionable" | "cell"
        );
        if !wanted {
            return None;
        }
        let card = match Self::text(args).and_then(|id| Self::card_of(state, id.trim())) {
            Ok(card) => card,
            Err(why) => return Some(Err(why)),
        };
        let id = card.id().as_str().to_string();
        let answer = match path {
            "title" => card.title().to_string(),
            "chrome" => offered_words(&card),
            "section" => def_for_card(&id).map_or_else(String::new, |d| d.section.to_string()),
            "state" => card.state().wire().to_string(),
            "detail" => card.state().detail().unwrap_or("").to_string(),
            "remedy" => remedy_word(card.remedy()).to_string(),
            // Where it is, or that it is nowhere on the board.
            "cell" => state.board.get().tile(card.id()).map_or_else(
                || "detached".to_string(),
                |t| format!("{},{},{},{}", t.col, t.row, t.w, t.h),
            ),
            _ => if card.remedy().is_some_and(Remedy::is_actionable) {
                "yes"
            } else {
                "no"
            }
            .to_string(),
        };
        Some(Ok(IntrospectValue::Text(answer)))
    }

    /// `act` — perform one header affordance on one card.
    ///
    /// The whole point of the header being a *set*: an affordance the card does
    /// not offer is refused **by name**, before anything happens. A shell that
    /// only hides the button leaves the wire able to do what the screen says is
    /// impossible.
    fn act(state: &Rc<ShellState>, args: &IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        let raw = Self::text(args)?;
        let (id, word) = raw
            .split_once(',')
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <card>,<affordance>")))?;
        let (id, word) = (id.trim(), word.trim());
        let card = Self::card_of(state, id)?;
        let affordance = CardAffordance::from_wire(word).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{word:?} is not an affordance; they are {}",
                CardAffordance::ALL.map(CardAffordance::wire).join(" / ")
            ))
        })?;
        if !card.chrome().offers(affordance) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} does not offer {word}; its header offers {}",
                offered_words(&card)
            )));
        }
        match affordance {
            CardAffordance::Settings => {
                let open = state.config_open.get().as_deref() == Some(id);
                state
                    .config_open
                    .set(if open { None } else { Some(id.to_string()) });
                state.say(format!(
                    "{} settings {}",
                    label_of(id),
                    if open { "closed" } else { "opened" }
                ));
            }
            CardAffordance::TearOff => return Self::detach(state, id),
            CardAffordance::Maximize => return Self::maximize(state, id),
            CardAffordance::Close => {
                Self::remove(state, id);
                state.say(format!("{} removed", label_of(id)));
            }
        }
        Ok(IntrospectValue::Text(format!("{id} {word}")))
    }

    /// Take a card off the board and out of the deck.
    fn remove(state: &Rc<ShellState>, id: &str) {
        let mut board = state.board.get();
        board.remove(&TileId::new(id)).ok();
        state.board.set(board);
        state.cards.set(
            state
                .cards
                .get()
                .into_iter()
                .filter(|c| c.id().as_str() != id)
                .collect(),
        );
        state.floats.set(
            state
                .floats
                .get()
                .into_iter()
                .filter(|f| f.id != id)
                .collect(),
        );
        if state.selected.get().as_deref() == Some(id) {
            state.selected.set(None);
        }
    }

    /// ★ Detach REMOVES the card from the board and opens a floating panel.
    ///
    /// R1648 kept the tile in place and argued that a temporary gesture should
    /// not reflow a layout. The reference tool does remove it, and re-docking
    /// appends at the bottom — so a detach IS a layout edit there, and on this
    /// axis the reference is the specification rather than a data point.
    fn detach(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if state.is_floating(id) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} is already detached"
            )));
        }
        Self::card_of(state, id)?;
        let mut board = state.board.get();
        board.remove(&TileId::new(id)).ok();
        state.board.set(board);
        let n = u(state.floats.get().len());
        let mut floats = state.floats.get();
        floats.push(Float {
            id: id.to_string(),
            x: 120 + n * FLOAT_STEP,
            y: 40 + n * FLOAT_STEP,
        });
        state.floats.set(floats);
        state.say(format!("{} \u{2192} detached window", label_of(id)));
        Ok(IntrospectValue::Text(format!("{id} tear_off")))
    }

    /// Put a detached card back — at the bottom of the board, as the reference
    /// does. The card lost its cell when it left, and inventing one back would
    /// be a third placement rule nobody asked for.
    fn redock(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if !state.is_floating(id) {
            return Err(InvokeError::rejected(format!(
                "card {id:?} is not detached"
            )));
        }
        let (cols, rows) = kind_span(kind_of(id))
            .ok_or_else(|| InvokeError::rejected(format!("no specified cell size for {id:?}")))?;
        let mut board = state.board.get();
        let row = board.rows();
        board
            .place(Tile::new(id, 0, row, cols, rows))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.floats.set(
            state
                .floats
                .get()
                .into_iter()
                .filter(|f| f.id != id)
                .collect(),
        );
        state.say(format!("{} re-docked", label_of(id)));
        Ok(IntrospectValue::Text(format!("{id} redock")))
    }

    /// Place a new card of that kind at the bottom of the board.
    fn add(state: &Rc<ShellState>, kind: &str) -> Result<IntrospectValue, InvokeError> {
        let def = def_of(kind.trim()).ok_or_else(|| {
            InvokeError::rejected(format!(
                "{kind:?} is not a widget kind; the palette offers {}",
                spec::CATALOGUE
                    .iter()
                    .filter(|w| w.tier == spec::Tier::Placeable)
                    .map(|w| w.kind)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        // R1668 — a reserved kind refuses, and the refusal carries the reason
        // rather than a bare "no". The palette paints the same fact (the row is
        // declared unavailable, so it is inert, faded and announced with its
        // reason), and this is that fact on the invoke path — the two cannot
        // drift because both read `reserved_for` from the specification.
        if def.tier == spec::Tier::Reserved {
            return Err(InvokeError::rejected(format!(
                "{:?} is reserved for {} and this release does not place it",
                def.kind, def.reserved_for
            )));
        }
        let (cols, rows) = kind_span(def.kind).ok_or_else(|| {
            InvokeError::rejected(format!("{:?} has no specified cell size", def.kind))
        })?;
        let ordinal = {
            let mut next = state.next_id.borrow_mut();
            let now = *next;
            *next += 1;
            now
        };
        let id = format!("{}#{ordinal}", def.kind);
        let mut board = state.board.get();
        let row = board.rows();
        board
            .place(Tile::new(id.clone(), 0, row, cols, rows))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        let mut cards = state.cards.get();
        cards.push(
            Card::new(id.clone(), def.label)
                .with_chrome(CardChrome::of(chrome()))
                .with_state(CardState::Ready),
        );
        state.cards.set(cards);
        state.selected.set(Some(id.clone()));
        state.say(format!("{} added", def.label));
        Ok(IntrospectValue::Text(id))
    }

    fn maximize(state: &Rc<ShellState>, id: &str) -> Result<IntrospectValue, InvokeError> {
        if state.maximized.get().is_some() {
            return Err(InvokeError::rejected(
                "a card is already maximised; restore first",
            ));
        }
        let mut board = state.board.get();
        let token = board
            .maximize(&TileId::new(id))
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.maximized.set(Some(token));
        state.say(format!("{} maximised", label_of(id)));
        Ok(IntrospectValue::Text(format!("{id} maximize")))
    }

    fn restore(state: &Rc<ShellState>) -> Result<IntrospectValue, InvokeError> {
        let token = state
            .maximized
            .get()
            .ok_or_else(|| InvokeError::rejected("no card is maximised"))?;
        let id = token.id().as_str().to_string();
        state.board.set(token.restore());
        state.maximized.set(None);
        state.say(format!("{} restored", label_of(&id)));
        Ok(IntrospectValue::Text(id))
    }

    /// A size stepper: one cell wider, narrower, taller or shorter.
    fn step(state: &Rc<ShellState>, id: &str, verb: &str) -> Result<IntrospectValue, InvokeError> {
        let tile_id = TileId::new(id);
        let mut board = state.board.get();
        let tile = board
            .tile(&tile_id)
            .ok_or_else(|| InvokeError::rejected(format!("card {id:?} is not on the board")))?;
        let (w, h) = match verb {
            "widen" => (tile.w + 1, tile.h),
            "narrow" => (tile.w.saturating_sub(1).max(1), tile.h),
            "taller" => (tile.w, tile.h + 1),
            "shorter" => (tile.w, tile.h.saturating_sub(1).max(1)),
            other => {
                return Err(InvokeError::rejected(format!(
                    "{other:?} is not a size step; they are {}",
                    STEPPERS.map(|(verb, _)| verb).join(" / ")
                )));
            }
        };
        board
            .resize(&tile_id, w, h)
            .map_err(|why| InvokeError::rejected(why.to_string()))?;
        state.board.set(board);
        state.say(format!("{} \u{2192} {w}\u{00D7}{h}", label_of(id)));
        Ok(IntrospectValue::Text(format!("{w}x{h}")))
    }

    fn set_state(
        state: &Rc<ShellState>,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let raw = Self::text(args)?;
        let mut parts = raw.splitn(3, ',');
        let id = parts.next().unwrap_or_default().trim();
        let word = parts
            .next()
            .ok_or_else(|| InvokeError::rejected(format!("{raw:?} is not <card>,<state>[,<why>]")))?
            .trim();
        let detail = parts.next().map(str::trim).filter(|d| !d.is_empty());
        Self::card_of(state, id)?;
        let next = parse_state(word, detail).map_err(InvokeError::rejected)?;
        let remedy = next.remedy();
        state.update_card(id, |card| card.set_state(next.clone()));
        state.say(format!("{} is {}", label_of(id), next.wire()));
        Ok(IntrospectValue::Text(format!(
            "{id} {} {}",
            next.wire(),
            remedy_word(remedy)
        )))
    }

    /// The app bar's own writable slots — source, capture, theme.
    ///
    /// `None` when `path` is not one of them, so the dispatcher stays a list.
    /// They are grouped because they are one region of the screen and each one
    /// refuses the same way: by naming what it does accept.
    fn write_bar(
        state: &Rc<ShellState>,
        path: &str,
        value: &IntrospectValue,
    ) -> Option<Result<(), InterveneError>> {
        let word = |value: &IntrospectValue| match value {
            IntrospectValue::Text(s) => Ok(s.trim().to_string()),
            _ => Err(InterveneError::TypeMismatch),
        };
        Some(match path {
            "source" => word(value).and_then(|name| {
                let chosen = SOURCES.iter().find(|s| **s == name).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a source; they are {}",
                        SOURCES.join(", ")
                    ))
                })?;
                state.source.set((*chosen).to_string());
                state.say(format!("source {chosen}"));
                Ok(())
            }),
            "capturing" => match value {
                IntrospectValue::Bool(on) => {
                    state.capturing.set(*on);
                    state.say(format!("capture {}", if *on { "on" } else { "off" }));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "theme" => word(value).and_then(|name| {
                let mode = match name.as_str() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    "system" => ThemeMode::System,
                    other => {
                        return Err(InterveneError::out_of_range(format!(
                            "{other:?} is not a theme; they are light / dark / system"
                        )));
                    }
                };
                state.theme.set_mode(mode);
                state.say(format!("theme {name}"));
                Ok(())
            }),
            _ => return None,
        })
    }

    fn apply_preset(state: &Rc<ShellState>, name: &str) -> Result<(), InterveneError> {
        let stored = state.presets.borrow().get(name).cloned();
        let preset = stored.ok_or_else(|| {
            InterveneError::out_of_range(format!(
                "{name:?} is not a saved layout; they are {}",
                state.preset_names()
            ))
        })?;
        // A preset restores BOTH facts. Restoring only the arrangement would
        // put the previous board's cards into the new layout's holes.
        state.maximized.set(None);
        state.floats.set(Vec::new());
        state.board.set(preset.board);
        state.cards.set(preset.cards);
        state.preset.set(name.to_string());
        state.preset_open.set(false);
        state.say(format!("layout \u{201C}{name}\u{201D}"));
        Ok(())
    }

    fn save_preset(state: &Rc<ShellState>, name: &str) -> Result<IntrospectValue, InvokeError> {
        if name.is_empty() {
            return Err(InvokeError::rejected("a layout preset needs a name"));
        }
        state.presets.borrow_mut().insert(
            name.to_string(),
            Preset {
                board: state.board.get(),
                cards: state.cards.get(),
            },
        );
        state.preset.set(name.to_string());
        state.say(format!("layout saved \u{00B7} {name}"));
        Ok(IntrospectValue::Text(state.preset_names()))
    }
}

fn remedy_word(remedy: Option<Remedy>) -> &'static str {
    remedy.map_or("none", Remedy::wire)
}

fn offered_words(card: &Card) -> String {
    card.chrome()
        .offered()
        .into_iter()
        .map(CardAffordance::wire)
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse `<state>` plus its optional detail into a [`CardState`].
///
/// The arity check is a fact about the vocabulary: two arms take a reason and
/// four do not.
fn parse_state(word: &str, detail: Option<&str>) -> Result<CardState, String> {
    let carried = |what: &str| -> Result<std::borrow::Cow<'static, str>, String> {
        detail
            .map(|d| std::borrow::Cow::Owned(d.to_string()))
            .ok_or_else(|| format!("{what} carries a reason: send \"<card>,{what},<why>\""))
    };
    let plain = |made: CardState| -> Result<CardState, String> {
        if detail.is_some() {
            return Err(format!(
                "{} carries no reason; the same explanation every time is not one",
                made.wire()
            ));
        }
        Ok(made)
    };
    match word {
        "ready" => plain(CardState::Ready),
        "loading" => plain(CardState::Loading),
        "empty" => plain(CardState::Empty),
        "opaque" => plain(CardState::Opaque),
        "failed" => Ok(CardState::Failed(carried("failed")?)),
        "denied" => Ok(CardState::Denied(carried("denied")?)),
        other => Err(format!(
            "{other:?} is not a card state; they are {}",
            CardState::ALL
                .iter()
                .map(CardState::wire)
                .collect::<Vec<_>>()
                .join(" / ")
        )),
    }
}

/// Everything this surface publishes.
const FIELDS: &[SchemaField] = const {
    &[
        // the app bar
        SchemaField::new("source", "string"),
        SchemaField::new("sources", "string"),
        SchemaField::new("capturing", "bool"),
        SchemaField::new("search", "string"),
        SchemaField::new("theme", "string"),
        SchemaField::new("tab", "string"),
        SchemaField::new("tabs", "string"),
        // R1668 — the reference screen this shell claims to reproduce, as the
        // table a gate reads. Published so the demo compares the running
        // application against the specification rather than against a second
        // copy of it (the failure R1649's sweep exists to prevent, one level up).
        SchemaField::new("spec", "json"),
        // the rail and the sub bar
        SchemaField::new("rail", "string"),
        SchemaField::new("reserved_rail", "json"),
        SchemaField::new("nav", "string"),
        SchemaField::new("editing", "bool"),
        SchemaField::new("config_open", "string"),
        // the catalogue and the board
        SchemaField::new("catalogue", "string"),
        SchemaField::new("cards", "string"),
        SchemaField::new("card_count", "int"),
        SchemaField::new("placed_count", "int"),
        SchemaField::new("layout", "string"),
        SchemaField::new("maximized", "string"),
        SchemaField::new("restore_to", "string"),
        SchemaField::new("floating", "string"),
        // named layouts
        SchemaField::new("preset", "string"),
        SchemaField::new("presets", "string"),
        SchemaField::new("preset_open", "bool"),
        // the transport
        SchemaField::new("transport", "string"),
        SchemaField::new("playhead", "int"),
        // the published vocabularies
        SchemaField::new("affordances", "string"),
        SchemaField::new("states", "string"),
        SchemaField::new("remedies", "string"),
        SchemaField::new("steppers", "string"),
        SchemaField::new("toast", "string"),
        // direct manipulation
        SchemaField::new("cursor", "string"),
        SchemaField::new("selected", "string"),
        SchemaField::new("hit", "string"),
        SchemaField::new("keymap", "string"),
        SchemaField::new("drag", "string"),
        // per-card reads
        SchemaField::action_with(
            "title",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "chrome",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "section",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "state",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "detail",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "remedy",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "actionable",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        SchemaField::action_with(
            "cell",
            "string",
            ArgForm::Scalar,
            const { &[SchemaArg::key("card", "string", "cards")] },
        ),
        // the verbs
        SchemaField::action_with(
            "act",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("affordance", "string", "affordances"),
                ]
            },
        ),
        SchemaField::action_with(
            "set_state",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("state", "string", "states"),
                    SchemaArg::key("why", "string", "states").optional(),
                ]
            },
        ),
        SchemaField::action_with(
            "resize",
            "string",
            ArgForm::Delimited(','),
            const {
                &[
                    SchemaArg::key("card", "string", "cards"),
                    SchemaArg::key("step", "string", "steppers"),
                ]
            },
        ),
        SchemaField::action("add", "string"),
        SchemaField::action("maximize", "string"),
        SchemaField::action("restore", "string"),
        SchemaField::action("redock", "string"),
        SchemaField::action("save_preset", "string"),
        SchemaField::action("seek", "string"),
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

impl ExternalIntrospect for ShellOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(FIELDS)
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| ReadRefusal::unavailable("no capture is loaded"))?;
        let text = |s: String| Ok(IntrospectValue::Text(s));
        let clock = &state.clock;
        match path {
            "source" => text(state.source.get()),
            "sources" => text(SOURCES.join(",")),
            "capturing" => Ok(IntrospectValue::Bool(state.capturing.get())),
            "search" => text(state.search.get()),
            "theme" => text(theme_word(&state.theme)),
            "tab" => text(state.tab.get()),
            "tabs" => text(TABS.join(",")),
            "spec" | "rail" | "reserved_rail" | "catalogue" => read_specification(path),
            "nav" => text(state.nav.get()),
            "editing" => Ok(IntrospectValue::Bool(state.editing.get())),
            "config_open" => text(state.config_open.get().unwrap_or_default()),
            "cards" => text(state.card_ids()),
            "card_count" => Ok(IntrospectValue::Int(i64::from(u(state.cards.get().len())))),
            "placed_count" => Ok(IntrospectValue::Int(i64::from(u(state.placed().len())))),
            "layout" => text(
                serde_json::to_string(&state.board.get()).unwrap_or_else(|why| why.to_string()),
            ),
            "maximized" => text(
                state
                    .maximized
                    .get()
                    .map_or_else(String::new, |m| m.id().as_str().to_string()),
            ),
            "restore_to" => text(state.maximized.get().map_or_else(String::new, |m| {
                serde_json::to_string(m.peek()).unwrap_or_else(|why| why.to_string())
            })),
            "floating" => text(
                state
                    .floats
                    .get()
                    .iter()
                    .map(|f| f.id.clone())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "preset" => text(state.preset.get()),
            "presets" => text(state.preset_names()),
            "preset_open" => Ok(IntrospectValue::Bool(state.preset_open.get())),
            "transport" => text(transport_word(clock.status(), state.capturing.get())),
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a playhead fraction is 0.0..=1.0, so per-mille is 0..=1000"
            )]
            "playhead" => Ok(IntrospectValue::Int(i64::from(
                (clock.position() * 1000.0).round() as i32,
            ))),
            "affordances" => text(CardAffordance::ALL.map(CardAffordance::wire).join(",")),
            "states" => text(
                CardState::ALL
                    .iter()
                    .map(CardState::wire)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "remedies" => text(Remedy::ALL.map(Remedy::wire).join(",")),
            "steppers" => text(STEPPERS.map(|(verb, _)| verb).join(",")),
            "toast" => text(state.toast.get()),
            "cursor" => {
                let (x, y) = state.cursor.get();
                text(format!("{x},{y}"))
            }
            "selected" => text(state.selected.get().unwrap_or_default()),
            "hit" => {
                let (x, y) = state.cursor.get();
                text(hit_word(&Hit::at(state, x, y)))
            }
            // Where a release would put the dragged card — the snap preview, as
            // a value. Empty when nothing is being dragged, which is a
            // different answer from a drag hovering over cell 0,0.
            "drag" => text(state.drag.get().map_or_else(String::new, |d| {
                format!("{},{},{}", d.id, d.snap.0, d.snap.1)
            })),
            "keymap" => text(
                KEYMAP
                    .iter()
                    .map(|(chord, what)| format!("{chord}={what}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let state = self
            .state
            .as_ref()
            .ok_or(InterveneError::UnknownPath)?
            .clone();
        let word = |value: &IntrospectValue| match value {
            IntrospectValue::Text(s) => Ok(s.trim().to_string()),
            _ => Err(InterveneError::TypeMismatch),
        };
        if let Some(done) = Self::write_bar(&state, path, &value) {
            return done;
        }
        match path {
            "editing" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.editing.set(on);
                state.say(if on {
                    "layout edit mode"
                } else {
                    "layout locked"
                });
                Ok(())
            }
            "preset_open" => {
                let IntrospectValue::Bool(on) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.preset_open.set(on);
                Ok(())
            }
            "search" => {
                let IntrospectValue::Text(needle) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                state.search.set(needle.clone());
                state.say(format!("search {needle:?}"));
                Ok(())
            }
            "tab" => {
                let name = word(&value)?;
                let chosen = TABS.iter().find(|t| **t == name).ok_or_else(|| {
                    InterveneError::out_of_range(format!(
                        "{name:?} is not a tab; they are {}",
                        TABS.join(", ")
                    ))
                })?;
                state.tab.set((*chosen).to_string());
                state.say(format!("view {chosen}"));
                Ok(())
            }
            "nav" => {
                let name = word(&value)?;
                let chosen = spec::RAIL
                    .iter()
                    .find(|seat| seat.key == name)
                    .ok_or_else(|| {
                        InterveneError::out_of_range(format!(
                            "{name:?} is not a rail section; they are {}",
                            spec::RAIL
                                .iter()
                                .map(|seat| seat.key)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })?;
                // R1668 — a reserved seat refuses, and says what it is waiting
                // for. The wire learns this the same way a person does: the
                // rail paints it inert and `scene/disabled` names the reason.
                if let Some(why) = chosen.reserved_for {
                    return Err(InterveneError::out_of_range(format!(
                        "the {name:?} section is reserved for {why}"
                    )));
                }
                state.nav.set(chosen.key.to_string());
                state.say(format!("{} section", chosen.title));
                Ok(())
            }
            "preset" => ShellOracle::apply_preset(&state, &word(&value)?),
            "sources" | "cards" | "card_count" | "placed_count" | "layout" | "maximized"
            | "restore_to" | "floating" | "presets" | "transport" | "playhead" | "affordances"
            | "states" | "remedies" | "steppers" | "toast" | "cursor" | "selected" | "hit"
            | "keymap" | "rail" | "tabs" | "catalogue" | "config_open" | "drag" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self.state()?.clone();
        if let Some(answer) = Self::card_read(&state, path, &args) {
            return answer;
        }
        match path {
            "act" => Self::act(&state, &args),
            "set_state" => Self::set_state(&state, &args),
            "add" => Self::add(&state, &Self::text(&args)?),
            "maximize" => Self::maximize(&state, Self::text(&args)?.trim()),
            "restore" => Self::restore(&state),
            "redock" => Self::redock(&state, Self::text(&args)?.trim()),
            "resize" => {
                let raw = Self::text(&args)?;
                let (id, verb) = raw.split_once(',').ok_or_else(|| {
                    InvokeError::rejected(format!("{raw:?} is not <card>,<step>"))
                })?;
                Self::step(&state, id.trim(), verb.trim())
            }
            "save_preset" => Self::save_preset(&state, Self::text(&args)?.trim()),
            "seek" => {
                let raw = Self::text(&args)?;
                let per_mille: i32 = raw
                    .trim()
                    .parse()
                    .map_err(|_| InvokeError::rejected(format!("{raw:?} is not 0..=1000")))?;
                if !(0..=1000).contains(&per_mille) {
                    return Err(InvokeError::rejected(format!(
                        "a playhead is 0..=1000 per mille, got {per_mille}"
                    )));
                }
                state.clock.pause();
                state
                    .clock
                    .seek(f32::from(i16::try_from(per_mille).unwrap_or(0)) / 1000.0);
                state.capturing.set(false);
                state.say(format!("seek {per_mille}"));
                Ok(IntrospectValue::Int(i64::from(per_mille)))
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
                if x >= WIN_W || y >= WIN_H {
                    return Err(InvokeError::rejected(format!(
                        "({x},{y}) is outside the {WIN_W}x{WIN_H} shell"
                    )));
                }
                Self::move_cursor(&state, x, y);
                Ok(IntrospectValue::Text(hit_word(&Hit::at(&state, x, y))))
            }
            "send" => {
                let event = Self::text(&args)?;
                match event.trim() {
                    "PointerDown" => Self::press(&state),
                    "PointerUp" => Self::release(&state),
                    // A cancel drops the latch WITHOUT performing it — the
                    // difference between letting go and being interrupted.
                    "PointerLeave" | "PointerCancel" => {
                        state.pressed.borrow_mut().take();
                        state.drag.set(None);
                    }
                    other => {
                        return Err(InvokeError::rejected(format!(
                            "{other:?} is not a pointer event; they are PointerDown / \
                             PointerUp / PointerLeave / PointerCancel"
                        )));
                    }
                }
                Ok(IntrospectValue::Text(state.toast.get()))
            }
            "key" => {
                let chord = Self::text(&args)?;
                Ok(IntrospectValue::Bool(Self::key(&state, chord.trim())))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- Direct manipulation -----------------------------------------------------

impl ShellOracle {
    /// Move the cursor, and update the snap preview if a drag is in flight.
    fn move_cursor(state: &Rc<ShellState>, px: u32, py: u32) {
        state.cursor.set((px, py));
        let Some(mut drag) = state.drag.get() else {
            return;
        };
        let canvas = canvas_rect();
        let (col, row) = cell_at(px.saturating_sub(canvas.x), py.saturating_sub(canvas.y));
        // ★ R1668 — the GRID says where it would land, rather than this file
        // guessing. A preview computed here and a release computed there is one
        // fact with two clamps, and the two disagreed: a six-column card
        // dragged to column seven previewed seven and committed six.
        let wanted = (col.saturating_sub(drag.dx), row.saturating_sub(drag.dy));
        let snap = state
            .board
            .get()
            .landing(&drag.id, wanted.0, wanted.1)
            .unwrap_or(wanted);
        if snap != drag.snap {
            drag.snap = snap;
            state.drag.set(Some(drag));
        }
    }

    /// A press latches what is under the cursor; a press on a card header opens
    /// a drag that **previews** rather than moving, because the reference
    /// commits on release and a board reflowing under the finger would make the
    /// preview a lie.
    fn press(state: &Rc<ShellState>) {
        let (px, py) = state.cursor.get();
        let hit = Hit::at(state, px, py);
        if let Some(id) = hit.card_id() {
            state.selected.set(Some(id.to_string()));
        }
        if let Hit::Grip(id) = &hit {
            let board = state.board.get();
            let tile_id = TileId::new(id.clone());
            if let Some(tile) = board.tile(&tile_id) {
                let canvas = canvas_rect();
                let (col, row) = cell_at(px.saturating_sub(canvas.x), py.saturating_sub(canvas.y));
                state.drag.set(Some(Drag {
                    id: tile_id,
                    dx: col.saturating_sub(tile.col),
                    dy: row.saturating_sub(tile.row),
                    snap: (tile.col, tile.row),
                }));
            }
        }
        *state.pressed.borrow_mut() = Some(hit);
    }

    /// A release performs the latched control if the cursor is still on it, and
    /// commits a drag wherever the preview ended up.
    fn release(state: &Rc<ShellState>) {
        let latched = state.pressed.borrow_mut().take();
        if let Some(drag) = state.drag.get() {
            state.drag.set(None);
            let mut board = state.board.get();
            if let Ok(reflow) = board.move_to(&drag.id, drag.snap.0, drag.snap.1) {
                state.board.set(board);
                state.say(if reflow.is_clean() {
                    format!("{} moved", label_of(drag.id.as_str()))
                } else {
                    format!(
                        "{} moved, displacing {}",
                        label_of(drag.id.as_str()),
                        reflow
                            .displaced()
                            .iter()
                            .map(|d| label_of(d.id.as_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                });
            }
            return;
        }
        let (px, py) = state.cursor.get();
        let Some(latched) = latched else { return };
        if Hit::at(state, px, py) != latched {
            return;
        }
        Self::act_on_hit(state, latched);
    }

    /// What a completed press on one hit target does.
    ///
    /// Lifted out of [`Self::release`] in R1668 so a test can put a hit to the
    /// shell directly. It was reachable only by placing a cursor and driving a
    /// press-release pair, which meant "does a reserved rail seat refuse" could
    /// be asked only through the geometry that already refuses to reach it --
    /// the shape R1649.1 named: a capability verified only by the path that
    /// bypasses it is a capability nobody verified.
    fn act_on_hit(state: &Rc<ShellState>, latched: Hit) {
        match latched {
            Hit::Chip(chip) => Self::press_chip(state, chip),
            Hit::Sub(chip) => Self::press_sub(state, chip),
            Hit::PresetItem(n) => Self::press_preset_item(state, n),
            Hit::Rail(key) => {
                let seat = spec::RAIL.iter().find(|seat| seat.key == key);
                let title = seat.map_or(key, |seat| seat.title);
                if let Some(why) = seat.and_then(|seat| seat.reserved_for) {
                    // The seat is painted inert, so a pointer never reaches it;
                    // this is the keyboard and wire path saying the same thing.
                    state.say(format!("{title} is reserved for {why}"));
                    return;
                }
                state.nav.set(key.to_string());
                state.say(format!("{title} section"));
            }
            Hit::Palette(kind) => {
                if let Err(why) = Self::add(state, kind) {
                    state.say(format!("refused: {why:?}"));
                }
            }
            Hit::Affordance(id, affordance) => {
                let call = IntrospectValue::Text(format!("{id},{}", affordance.wire()));
                if let Err(why) = Self::act(state, &call) {
                    // A refusal a person triggered has to be visible to that
                    // person, not only to the wire that would have read it.
                    state.say(format!("refused: {why:?}"));
                }
            }
            Hit::Stepper(id, verb) => {
                if let Err(why) = Self::step(state, &id, verb) {
                    state.say(format!("refused: {why:?}"));
                }
            }
            Hit::Remedy(id) => Self::apply_remedy(state, &id),
            Hit::FloatRedock(id) => {
                if let Err(why) = Self::redock(state, &id) {
                    state.say(format!("refused: {why:?}"));
                }
            }
            Hit::FloatClose(id) => {
                Self::remove(state, &id);
                state.say(format!("{} closed", label_of(&id)));
            }
            Hit::Card(id) | Hit::Grip(id) => state.say(format!("{} selected", label_of(&id))),
            Hit::Float(_) | Hit::Nothing => {}
        }
    }

    fn press_chip(state: &Rc<ShellState>, chip: BarChip) {
        match chip {
            BarChip::Tab0 | BarChip::Tab1 => {
                let name = if chip == BarChip::Tab0 {
                    TABS[0]
                } else {
                    TABS[1]
                };
                state.tab.set(name.to_string());
                state.say(format!("view {name}"));
            }
            BarChip::Source => {
                let now = state.source.get();
                let at = SOURCES.iter().position(|s| *s == now).unwrap_or(0);
                let next = SOURCES[(at + 1) % SOURCES.len()];
                state.source.set(next.to_string());
                state.say(format!("source {next}"));
            }
            BarChip::Capture => {
                let on = !state.capturing.get();
                state.capturing.set(on);
                state.say(format!("capture {}", if on { "on" } else { "off" }));
            }
            BarChip::Search => {
                state.searching.set(true);
                state.say("searching (Enter or Escape leaves)");
            }
        }
    }

    fn press_sub(state: &Rc<ShellState>, chip: SubChip) {
        match chip {
            SubChip::Preset => {
                let open = !state.preset_open.get();
                state.preset_open.set(open);
            }
            SubChip::EditLayout => {
                let on = !state.editing.get();
                state.editing.set(on);
                state.say(if on {
                    "layout edit mode"
                } else {
                    "layout locked"
                });
            }
            SubChip::AddWidget => {
                // The palette is always open in this shell, so the button is
                // what SAYS where widgets come from: it selects the catalogue
                // section rather than opening a second chooser.
                state.nav.set("catalog".to_string());
                state.say("pick a widget from the palette \u{2192}");
            }
        }
    }

    /// The preset menu's rows: every saved layout, then "Save current layout".
    fn press_preset_item(state: &Rc<ShellState>, n: usize) {
        let names: Vec<String> = state.presets.borrow().keys().cloned().collect();
        if let Some(name) = names.get(n) {
            ShellOracle::apply_preset(state, name).ok();
            return;
        }
        // The last row saves. The name is derived rather than typed, because a
        // text prompt is a modal this shell has no business inventing.
        let name = format!("Layout {}", names.len() + 1);
        Self::save_preset(state, &name).ok();
        state.preset_open.set(false);
    }

    /// What pressing an actionable remedy does.
    ///
    /// The framework decides WHICH remedy a state has; what a remedy MEANS for
    /// this data is the application's, which is why this lives here and
    /// `Remedy` has no `perform`.
    fn apply_remedy(state: &Rc<ShellState>, id: &str) {
        let Some(card) = state.card(id) else { return };
        let Some(remedy) = card.remedy().filter(|r| r.is_actionable()) else {
            state.say(format!("{}: nothing to do about this", label_of(id)));
            return;
        };
        let next = match remedy {
            Remedy::Retry => CardState::Loading,
            Remedy::Widen => {
                state.search.set(String::new());
                CardState::Ready
            }
            Remedy::Authorize => CardState::Ready,
            Remedy::Wait | Remedy::Nothing => return,
        };
        state.update_card(id, |card| card.set_state(next.clone()));
        state.say(format!(
            "{}: {} \u{2192} {}",
            label_of(id),
            remedy.wire(),
            next.wire()
        ));
    }

    /// The keymap, as one function so the wire and a real keyboard drive the
    /// same one rather than two that drift.
    fn key(state: &Rc<ShellState>, chord: &str) -> bool {
        if state.searching.get() {
            return Self::search_key(state, chord);
        }
        if chord == "/" {
            state.searching.set(true);
            state.say("searching (Enter or Escape leaves)");
            return true;
        }
        let selected = state.selected.get();
        let (modifiers, base) = chord.rsplit_once('+').map_or(("", chord), |(m, b)| (m, b));
        let shift = modifiers.contains("Shift");
        let alt = modifiers.contains("Alt");
        let direction = match base {
            "ArrowLeft" => Some(TileDirection::Left),
            "ArrowRight" => Some(TileDirection::Right),
            "ArrowUp" => Some(TileDirection::Up),
            "ArrowDown" => Some(TileDirection::Down),
            _ => None,
        };
        if let Some(direction) = direction {
            return Self::arrow(state, direction, shift, alt);
        }
        match base {
            "Enter" => selected.is_some_and(|id| {
                if state.maximized.get().is_some() {
                    Self::restore(state).is_ok()
                } else {
                    Self::maximize(state, &id).is_ok()
                }
            }),
            "Escape" => {
                if state.preset_open.get() {
                    state.preset_open.set(false);
                    return true;
                }
                Self::restore(state).is_ok()
            }
            "Delete" | "Backspace" => selected.is_some_and(|id| {
                Self::act(state, &IntrospectValue::Text(format!("{id},close"))).is_ok()
            }),
            "e" | "E" => {
                Self::press_sub(state, SubChip::EditLayout);
                true
            }
            "o" | "O" => selected.is_some_and(|id| {
                if state.is_floating(&id) {
                    Self::redock(state, &id).is_ok()
                } else {
                    Self::act(state, &IntrospectValue::Text(format!("{id},tear_off"))).is_ok()
                }
            }),
            "r" | "R" => selected.is_some_and(|id| {
                Self::apply_remedy(state, &id);
                true
            }),
            "c" | "C" => {
                Self::press_chip(state, BarChip::Capture);
                true
            }
            "t" | "T" => {
                let dark = theme_word(&state.theme) != "dark";
                state.theme.set_mode(if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                });
                state.say(format!("theme {}", if dark { "dark" } else { "light" }));
                true
            }
            "s" | "S" => {
                Self::press_chip(state, BarChip::Source);
                true
            }
            _ => false,
        }
    }

    /// Keystrokes while the search box has them. Deliberately narrow, and
    /// everything else is REFUSED rather than falling through to the board,
    /// because a chord that quietly did something else while the caret was in a
    /// text box is the worst thing a mode can do.
    fn search_key(state: &Rc<ShellState>, chord: &str) -> bool {
        match chord {
            "Enter" | "Escape" => {
                state.searching.set(false);
                state.say(format!("search {:?}", state.search.get()));
                true
            }
            "Backspace" => {
                let mut text = state.search.get();
                let had = text.pop().is_some();
                state.search.set(text);
                had
            }
            "Space" => {
                state.search.set(format!("{} ", state.search.get()));
                true
            }
            one if one.chars().count() == 1 && !one.chars().any(char::is_control) => {
                state.search.set(format!("{}{one}", state.search.get()));
                true
            }
            _ => false,
        }
    }

    /// The four arrows, and the three things a modifier makes them mean.
    fn arrow(state: &Rc<ShellState>, direction: TileDirection, shift: bool, alt: bool) -> bool {
        let Some(id) = state.selected.get() else {
            // Nothing selected: the first arrow picks a card, so the keyboard
            // has a way in that does not require a click.
            let first = state.placed().first().map(|c| c.id().as_str().to_string());
            let had = first.is_some();
            state.selected.set(first);
            return had;
        };
        let tile = TileId::new(id.clone());
        if !shift && !alt {
            let board = state.board.get();
            let Some(next) = board.neighbour(&tile, direction).map(|t| t.id.clone()) else {
                return false;
            };
            state.selected.set(Some(next.as_str().to_string()));
            state.say(format!("{} selected", label_of(next.as_str())));
            return true;
        }
        let nudge = match (shift, alt) {
            (true, false) => TileNudge::Move(direction),
            (false, true) => TileNudge::Grow(direction),
            _ => TileNudge::Shrink(direction),
        };
        let mut board = state.board.get();
        match board.nudge(&tile, nudge) {
            Ok(_) => {
                state.board.set(board);
                state.say(format!("{} {nudge:?}", label_of(&id)));
                true
            }
            Err(why) => {
                state.say(format!("refused: {why}"));
                false
            }
        }
    }
}

impl External for ShellOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Keep the cursor while a press is held, so a drag that strays off a card
    /// keeps previewing rather than being cancelled by a stray pixel.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Hover positions too: the shell tracks the cursor so a press knows where
    /// it landed, and the press event itself carries no coordinates.
    fn wants_hover_move(&self) -> bool {
        true
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window fraction times a window size is a pixel inside it"
    )]
    /// R1656 §5.15 — the shell's resize notification, which is how this surface
    /// knows what a pointer fraction is a fraction OF.
    fn on_resize(&mut self, width: u32, height: u32) {
        self.surface = (width.max(1), height.max(1));
    }

    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let Some(state) = self.state.clone() else {
            return;
        };
        // ★ R1656 — the LIVE surface, told by `External::on_resize`. It was the
        // design constant, which is right at the size the app opens in and
        // wrong by opening-size-over-current-size at every other size: a person
        // reported nodes that stop clicking after a maximise, and the
        // coordinates were measured arriving at 0.5775x.
        let (sw, sh) = self.surface;
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a clamped 0..=1 fraction times a window size is a pixel inside it"
        )]
        let (px, py) = (
            (x_rel.clamp(0.0, 1.0) * sw as f32) as u32,
            (y_rel.clamp(0.0, 1.0) * sh as f32) as u32,
        );
        Self::move_cursor(&state, px, py);
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

fn theme_word(theme: &ThemeProvider) -> String {
    match theme.mode() {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
        // `ThemeMode` is `#[non_exhaustive]`: a mode this shell has not been
        // taught is reported as itself rather than folded into one of the
        // three, which would make the app bar claim a setting nobody chose.
        other => return format!("{other:?}").to_lowercase(),
    }
    .to_string()
}

/// The three transport words, derived rather than stored: "live" is not a
/// fourth clock state, it is the absence of a replay while capture is on.
fn transport_word(status: TransportStatus, capturing: bool) -> String {
    match status {
        TransportStatus::Playing => "replaying",
        TransportStatus::Stopped if capturing => "live",
        TransportStatus::Paused | TransportStatus::Stopped => "paused",
    }
    .to_string()
}

// --- The view ----------------------------------------------------------------

/// The colours every painter here reads.
#[derive(Debug, Clone, Copy)]
struct Palette {
    ink: Color,
    muted: Color,
    accent: Color,
    on_accent: Color,
    accent_fg: Color,
    canvas: Color,
    panel: Color,
    raised: Color,
    high: Color,
    outline: Color,
    grid: Color,
    /// R1668 — the ink an identifier the capture cannot resolve is drawn in.
    /// A role rather than a literal because a warning has to hold its contrast
    /// in both themes, which a hand-picked amber does in exactly one.
    warn: Color,
}

/// A text run at an exact place in its container.
///
/// ★ The `with_layout` is not optional. A `TextNode` carries a rect, but
/// without a layout style it is laid out **in flow** by the parent — so a set
/// of labels written at deliberate coordinates stacks vertically instead, and
/// the screen reads as a list of everything the container holds. This shell's
/// first draft omitted it and every card painted its title, badge and
/// description down the left edge.
fn label(text: &str, rect: Rect, px: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(text, rect, TextStyle::new().with_size_px(px).with_fg(fg))
            .with_layout(absolute(rect)),
    )
}

/// Place a node at an exact rectangle inside its container, and make it
/// **pointer-transparent**.
///
/// ★★ The transparency is load-bearing and it is the bug this shell shipped.
/// The §5.35 input router resolves the hit target by hit-testing the paint
/// scene for the DEEPEST TAGGED node under the cursor, then looks up an
/// `External` carrying that tag. Every tag here is an ADDRESS — the thing the
/// wire and the demo compare (R1613: a tag is an address, not a claim of
/// clickability) — and there is exactly one `External`, the root. So a tagged
/// child that is not transparent becomes the target, the lookup finds no
/// `External` with that tag, and the router silently forwards NOTHING: a real
/// mouse move never arrives and every control is dead to a hand, while the
/// wire's own `point` / `send` keep working because they bypass the router.
///
/// That asymmetry is why the first version passed its whole demo while being
/// unusable, and it is exactly the "parallel automation surface" the module
/// docs claim does not exist. The root container is the one node that keeps
/// its own layout, so it stays the target.
fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w, rect.h))
        .with_pointer_transparent(true)
}

/// A small square dot — a grid pip, a status light, a grip dot.
fn dot(x: u32, y: u32, size: u32, fill: Color) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(size / 2))
            .with_layout(absolute(Rect::new(x, y, size, size))),
    )
}

#[allow(
    clippy::cast_precision_loss,
    reason = "shell coordinates are < 2^13, exactly representable in f32"
)]
fn ppt(x: u32, y: u32) -> PathPoint {
    PathPoint::new(x as f32, y as f32)
}

/// A stroked polyline set, in `rect`-local coordinates.
fn strokes(rect: Rect, runs: &[Vec<(u32, u32)>], ink: Color, width: u32) -> Scene {
    let mut commands = Vec::new();
    for run in runs {
        for (n, (x, y)) in run.iter().enumerate() {
            let point = ppt(*x, *y);
            commands.push(if n == 0 {
                PathCommand::MoveTo(point)
            } else {
                PathCommand::LineTo(point)
            });
        }
    }
    Scene::Path(
        PathNode::new(rect, commands, PathStyle::stroked(Stroke::new(ink, width)))
            .with_layout(absolute(rect)),
    )
}

fn close_mark(rect: Rect, ink: Color) -> Scene {
    let (w, h) = (rect.w, rect.h);
    let (x0, y0, x1, y1) = (w / 2 - 4, h / 2 - 4, w / 2 + 4, h / 2 + 4);
    strokes(
        rect,
        &[vec![(x0, y0), (x1, y1)], vec![(x1, y0), (x0, y1)]],
        ink,
        1,
    )
}

/// The detach mark: a square lifting out of another.
fn detach_mark(rect: Rect, ink: Color) -> Scene {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    strokes(
        rect,
        &[
            vec![
                (cx - 5, cy - 1),
                (cx - 5, cy + 5),
                (cx + 1, cy + 5),
                (cx + 1, cy - 1),
                (cx - 5, cy - 1),
            ],
            vec![(cx - 1, cy - 5), (cx + 5, cy - 5), (cx + 5, cy + 1)],
        ],
        ink,
        1,
    )
}

/// The re-dock mark: a box with a bar along its foot.
fn redock_mark(rect: Rect, ink: Color) -> Scene {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    strokes(
        rect,
        &[
            vec![
                (cx - 5, cy - 5),
                (cx + 5, cy - 5),
                (cx + 5, cy + 5),
                (cx - 5, cy + 5),
                (cx - 5, cy - 5),
            ],
            vec![(cx - 5, cy + 2), (cx + 5, cy + 2)],
        ],
        ink,
        1,
    )
}

/// One header control's mark.
fn affordance_mark(affordance: CardAffordance, rect: Rect, ink: Color) -> Vec<Scene> {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match affordance {
        CardAffordance::Settings => (0..3)
            .map(|n| dot(cx - 1, cy - 5 + n * 5, 2, ink))
            .collect(),
        CardAffordance::TearOff => vec![detach_mark(rect, ink)],
        CardAffordance::Maximize => vec![strokes(
            rect,
            &[vec![
                (cx - 5, cy - 5),
                (cx + 5, cy - 5),
                (cx + 5, cy + 5),
                (cx - 5, cy + 5),
                (cx - 5, cy - 5),
            ]],
            ink,
            1,
        )],
        CardAffordance::Close => vec![close_mark(rect, ink)],
    }
}

/// The rail's icon for one section, drawn rather than set in a font — a glyph
/// this project does not ship is a box, and a box is not an icon.
fn rail_mark(key: &str, rect: Rect, ink: Color) -> Vec<Scene> {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match key {
        "dashboard" => vec![
            dot(cx - 6, cy - 6, 5, ink),
            dot(cx + 1, cy - 6, 5, ink),
            dot(cx - 6, cy + 1, 5, ink),
            dot(cx + 1, cy + 1, 5, ink),
        ],
        "topology" => vec![
            strokes(
                rect,
                &[
                    vec![(cx, cy - 4), (cx, cy + 1)],
                    vec![
                        (cx - 5, cy + 5),
                        (cx - 5, cy + 1),
                        (cx + 5, cy + 1),
                        (cx + 5, cy + 5),
                    ],
                ],
                ink,
                1,
            ),
            dot(cx - 2, cy - 8, 4, ink),
        ],
        "stream" => vec![strokes(
            rect,
            &[
                vec![(cx - 7, cy - 4), (cx + 7, cy - 4)],
                vec![(cx - 7, cy), (cx + 3, cy)],
                vec![(cx - 7, cy + 4), (cx + 6, cy + 4)],
            ],
            ink,
            1,
        )],
        "decode" => vec![strokes(
            rect,
            &[
                vec![(cx - 2, cy - 5), (cx - 7, cy), (cx - 2, cy + 5)],
                vec![(cx + 2, cy - 5), (cx + 7, cy), (cx + 2, cy + 5)],
            ],
            ink,
            1,
        )],
        "catalog" => vec![
            dot(cx - 7, cy - 5, 3, ink),
            dot(cx - 7, cy - 1, 3, ink),
            dot(cx - 7, cy + 3, 3, ink),
            strokes(
                rect,
                &[
                    vec![(cx - 2, cy - 4), (cx + 7, cy - 4)],
                    vec![(cx - 2, cy), (cx + 7, cy)],
                    vec![(cx - 2, cy + 4), (cx + 7, cy + 4)],
                ],
                ink,
                1,
            ),
        ],
        _ => vec![
            strokes(
                rect,
                &[
                    vec![(cx - 7, cy - 4), (cx + 7, cy - 4)],
                    vec![(cx - 7, cy + 3), (cx + 7, cy + 3)],
                ],
                ink,
                1,
            ),
            dot(cx - 2, cy - 6, 4, ink),
            dot(cx + 2, cy + 1, 4, ink),
        ],
    }
}

/// A pill: a rounded box with a status light and a label.
fn pill(rect: Rect, tag: &str, light: Color, text: &str, palette: Palette) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![
            dot(12, rect.h / 2 - 3, 7, light),
            label(
                text,
                Rect::new(28, rect.h / 2 - 8, rect.w.saturating_sub(38), 16),
                FONT_BODY,
                palette.ink,
            ),
        ])
        .with_tag(tag.to_string())
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(8)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(rect)),
    )
}

/// A button: filled when it is the affirmative one, outlined otherwise.
fn button(rect: Rect, tag: &str, text: &str, filled: bool, palette: Palette) -> Scene {
    let (fill, ink, border) = if filled {
        (palette.accent, palette.on_accent, palette.accent)
    } else {
        (palette.panel, palette.ink, palette.outline)
    };
    Scene::Container(
        ContainerNode::new(vec![label(
            text,
            Rect::new(14, rect.h / 2 - 8, rect.w.saturating_sub(24), 16),
            FONT_BODY,
            ink,
        )])
        .with_tag(tag.to_string())
        .with_style(
            BoxStyle::filled(fill)
                .with_corner_radius(8)
                .with_border(Border::new(border, 1)),
        )
        .with_layout(absolute(rect)),
    )
}

fn app_bar_scene(state: &ShellState, palette: Palette) -> Scene {
    let mut children = vec![
        dot(16, 18, 16, palette.accent),
        label(
            "Analyzer",
            Rect::new(42, 17, 118, 18),
            FONT_TITLE,
            palette.ink,
        ),
    ];
    for (n, name) in TABS.iter().enumerate() {
        let chip = if n == 0 { BarChip::Tab0 } else { BarChip::Tab1 };
        let on = state.tab.get() == *name;
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                name,
                Rect::new(14, 8, chip.rect().w.saturating_sub(20), 16),
                FONT_BODY,
                if on { palette.ink } else { palette.muted },
            )])
            .with_tag(chip.tag())
            .with_style(
                BoxStyle::filled(if on { palette.high } else { palette.panel })
                    .with_corner_radius(8),
            )
            .with_layout(absolute(chip.rect())),
        ));
    }
    children.push(pill(
        BarChip::Source.rect(),
        BarChip::Source.tag(),
        rgb(0x35_C0_8B),
        &state.source.get(),
        palette,
    ));
    let capturing = state.capturing.get();
    children.push(pill(
        BarChip::Capture.rect(),
        BarChip::Capture.tag(),
        if capturing {
            palette.accent_fg
        } else {
            palette.muted
        },
        if capturing { spec::TRANSPORT } else { "Paused" },
        palette,
    ));
    children.push(label(
        &transport_word(state.clock.status(), capturing),
        Rect::new(842, 19, 92, 16),
        FONT_SMALL,
        palette.muted,
    ));
    // The rate readout: what a capture tool is counting while it runs.
    children.push(label(
        spec::RATE,
        Rect::new(938, 19, 96, 16),
        FONT_SMALL,
        palette.muted,
    ));
    // The search box says which of its two states it is in; a box that looked
    // the same either way would leave a person typing into the board.
    let searching = state.searching.get();
    let search = state.search.get();
    children.push(Scene::Container(
        ContainerNode::new(vec![label(
            &if searching {
                format!("{search}|")
            } else if search.is_empty() {
                spec::SEARCH_HINT.to_string()
            } else {
                search
            },
            Rect::new(12, 8, BarChip::Search.rect().w.saturating_sub(20), 16),
            FONT_BODY,
            if searching {
                palette.ink
            } else {
                palette.muted
            },
        )])
        .with_tag(BarChip::Search.tag())
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(8)
                .with_border(Border::new(
                    if searching {
                        palette.accent_fg
                    } else {
                        palette.outline
                    },
                    1,
                )),
        )
        .with_layout(absolute(BarChip::Search.rect())),
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("shell.appbar")
            .with_style(BoxStyle::filled(palette.panel))
            .with_layout(absolute(Rect::new(0, 0, WIN_W, APP_BAR_H))),
    )
}

fn sub_bar_scene(state: &ShellState, palette: Palette) -> Scene {
    let placed = state.placed().len();
    let preset = SubChip::Preset.rect();
    let mut children = vec![
        Scene::Container(
            ContainerNode::new(vec![
                label(
                    &state.preset.get(),
                    Rect::new(12, 8, preset.w.saturating_sub(38), 16),
                    FONT_TITLE,
                    palette.ink,
                ),
                strokes(
                    Rect::new(preset.w - 26, 13, 12, 8),
                    &[vec![(0, 0), (5, 5), (10, 0)]],
                    palette.muted,
                    1,
                ),
            ])
            .with_tag(SubChip::Preset.tag())
            .with_style(BoxStyle::filled(palette.panel).with_corner_radius(8))
            .with_layout(absolute(preset)),
        ),
        label(
            &format!("{placed} widgets placed"),
            Rect::new(preset.x + preset.w + 14, preset.y + 8, 220, 16),
            FONT_BODY,
            palette.muted,
        ),
        button(
            SubChip::EditLayout.rect(),
            SubChip::EditLayout.tag(),
            if state.editing.get() {
                "Done"
            } else {
                spec::BOARD_VERBS[0]
            },
            state.editing.get(),
            palette,
        ),
        button(
            SubChip::AddWidget.rect(),
            SubChip::AddWidget.tag(),
            spec::BOARD_VERBS[1],
            true,
            palette,
        ),
    ];
    if state.preset_open.get() {
        children.push(preset_menu_scene(state, palette));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("shell.subbar")
            .with_style(BoxStyle::filled(palette.canvas))
            .with_layout(absolute(Rect::new(
                RAIL_W,
                APP_BAR_H,
                WIN_W - RAIL_W - PALETTE_W,
                SUB_BAR_H,
            ))),
    )
}

/// The saved-layout menu, painted in the sub bar's own space — the same space
/// the hit test resolves its rows in.
fn preset_menu_scene(state: &ShellState, palette: Palette) -> Scene {
    let names: Vec<String> = state.presets.borrow().keys().cloned().collect();
    let rows = u(names.len()) + 1;
    let first = preset_item_rect(0);
    let panel = Rect::new(first.x - 8, first.y - 30, first.w + 16, rows * 34 + 38);
    let mut children = vec![label(
        "SAVED LAYOUTS",
        Rect::new(14, 10, panel.w - 24, 14),
        FONT_TINY,
        palette.muted,
    )];
    let row_local = |row: Rect| Rect::new(row.x - panel.x, row.y - panel.y, row.w, row.h);
    for (n, name) in names.iter().enumerate() {
        let row = preset_item_rect(u(n));
        let on = &state.preset.get() == name;
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                name,
                Rect::new(12, 7, row.w.saturating_sub(20), 16),
                FONT_BODY,
                if on { palette.accent_fg } else { palette.ink },
            )])
            .with_tag(format!("shell.preset.item.{n}"))
            .with_style(
                BoxStyle::filled(if on { palette.high } else { palette.raised })
                    .with_corner_radius(6),
            )
            .with_layout(absolute(row_local(row))),
        ));
    }
    let save = preset_item_rect(u(names.len()));
    children.push(Scene::Container(
        ContainerNode::new(vec![label(
            "+  Save current layout",
            Rect::new(12, 7, save.w.saturating_sub(20), 16),
            FONT_BODY,
            palette.accent_fg,
        )])
        .with_tag(format!("shell.preset.item.{}", names.len()))
        .with_style(BoxStyle::filled(palette.raised).with_corner_radius(6))
        .with_layout(absolute(row_local(save))),
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("shell.preset.menu")
            .with_style(
                BoxStyle::filled(palette.panel)
                    .with_corner_radius(10)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(panel)),
    )
}

fn rail_scene(state: &ShellState, palette: Palette) -> Scene {
    let mut entries = Vec::new();
    let nav = state.nav.get();
    for (n, seat) in spec::RAIL.iter().enumerate() {
        let key = seat.key;
        let rect = rail_rect(u(n));
        let on = nav == key;
        let ink = if on { palette.accent_fg } else { palette.muted };
        // R1668 — a reserved seat is DECLARED unavailable rather than painted
        // grey by hand. The declaration is what makes it inert to the pointer,
        // fades its ink, announces it to a screen reader and puts the reason on
        // `scene/disabled`; a hand-picked grey would do only the last of those,
        // and would do it in a way nothing can check.
        let layout = seat.reserved_for.map_or_else(
            || absolute(rect),
            |why| absolute(rect).with_unavailable(Unavailable::reserved(why)),
        );
        entries.push(Scene::Container(
            ContainerNode::new(rail_mark(key, local(rect), ink))
                .with_tag(format!("shell.rail.{key}"))
                .with_style(
                    BoxStyle::filled(if on { palette.high } else { palette.panel })
                        .with_corner_radius(8),
                )
                .with_layout(layout),
        ));
    }
    entries.push(Scene::Container(
        ContainerNode::new(vec![label(
            "NE",
            Rect::new(8, 9, 24, 14),
            FONT_TINY,
            palette.on_accent,
        )])
        .with_tag("shell.rail.account")
        .with_style(BoxStyle::filled(palette.accent).with_corner_radius(16))
        .with_layout(absolute(Rect::new(10, WIN_H - APP_BAR_H - 46, 32, 32))),
    ));
    Scene::Container(
        ContainerNode::new(entries)
            .with_tag("shell.rail")
            .with_style(BoxStyle::filled(palette.panel))
            .with_layout(absolute(Rect::new(
                0,
                APP_BAR_H,
                RAIL_W,
                WIN_H.saturating_sub(APP_BAR_H),
            ))),
    )
}

/// The canvas's dot grid: one pip per cell corner.
///
/// Painted always, faintly — an empty board that looked like an empty panel
/// would not say that things can be placed on it — and brighter while a drag or
/// layout-edit is in flight, which is the reference's aligned overlay.
fn grid_scene(rows: u32, palette: Palette, bright: bool) -> Vec<Scene> {
    let pitch = col_pitch();
    let ink = if bright {
        palette.outline
    } else {
        palette.grid
    };
    let size = if bright { 3 } else { 2 };
    let mut out = Vec::new();
    for row in 0..=rows.max(1) {
        for col in 0..=GRID_COLS {
            out.push(dot(
                GAP + col * pitch - size / 2,
                GAP + row * ROW_H - size / 2,
                size,
                ink,
            ));
        }
    }
    out
}

/// A card's header: grip, status light, title, LIVE badge, controls.
fn header_scene(card: &Card, rect: Rect, palette: Palette) -> Vec<Scene> {
    let id = card.id().as_str();
    let colour = kind_color(kind_of(id));
    let grip = grip_rect(rect);
    let mut out = vec![Scene::Container(
        ContainerNode::new(
            (0..3)
                .flat_map(|r| (0..2).map(move |c| dot(4 + c * 5, 8 + r * 5, 2, palette.muted)))
                .collect(),
        )
        .with_tag(format!("card.{id}.grip"))
        .with_layout(absolute(grip)),
    )];
    out.push(dot(
        grip.x + grip.w + 4,
        rect.y + CARD_HDR / 2 - 4,
        9,
        colour,
    ));
    let offered = card.chrome().offered();
    let title_w = rect
        .w
        .saturating_sub(grip.w + 32 + u(offered.len()) * SLOT_W + 56)
        .max(40);
    out.push(label(
        card.title(),
        Rect::new(grip.x + grip.w + 20, rect.y + 9, title_w, 16),
        FONT_BODY,
        palette.ink,
    ));
    if card.state().is_ready() {
        let badge_x = grip.x + grip.w + 24 + title_w;
        out.push(dot(
            badge_x,
            rect.y + CARD_HDR / 2 - 3,
            6,
            palette.accent_fg,
        ));
        out.push(label(
            "LIVE",
            Rect::new(badge_x + 10, rect.y + 10, 40, 14),
            FONT_TINY,
            palette.accent_fg,
        ));
    }
    for (n, affordance) in offered.iter().enumerate() {
        let slot = affordance_rect(rect, u(offered.len()), u(n));
        out.push(Scene::Container(
            ContainerNode::new(affordance_mark(*affordance, local(slot), palette.muted))
                .with_tag(format!("card.{id}.{}", affordance.wire()))
                .with_layout(absolute(slot)),
        ));
    }
    out
}

/// What a card's body paints: its content, or — for **every** not-ready
/// state — the same two things, the sentence and its derived remedy, so the
/// twelve kinds cannot disagree about what an encrypted link offers.
fn body_scene(card: &Card, rect: Rect, palette: Palette) -> Vec<Scene> {
    if card.state().is_ready() {
        return ready_body(card, rect, palette);
    }
    let mut out = vec![label(
        &state_sentence(card.state()),
        Rect::new(rect.x + 12, rect.y + 10, rect.w.saturating_sub(24), 16),
        FONT_BODY,
        palette.muted,
    )];
    if let Some(remedy) = card.remedy() {
        // A remedy is painted as a control only when the person is the one
        // expected to act. `Wait` is the card's own job and `Nothing` is
        // nobody's, and neither gets a button — the derivation doing the
        // deciding rather than this function.
        let slot = remedy_rect(rect);
        let actionable = remedy.is_actionable();
        out.push(Scene::Container(
            ContainerNode::new(vec![label(
                remedy_label(remedy),
                Rect::new(12, 3, slot.w.saturating_sub(20), 16),
                FONT_BODY,
                if actionable {
                    palette.on_accent
                } else {
                    palette.muted
                },
            )])
            .with_tag(format!("card.{}.remedy", card.id().as_str()))
            .with_style(
                BoxStyle::filled(if actionable {
                    palette.accent
                } else {
                    palette.raised
                })
                .with_corner_radius(6),
            )
            .with_layout(absolute(slot)),
        ));
    }
    out
}

/// A ready card's content — the body the specification gives that kind.
///
/// R1668 replaced a placeholder here. The four placeable widgets are what
/// screen C *is*, and a board of four coloured swatches reproduces the
/// arrangement while reproducing none of the screen: the gate next door
/// compares painted rows against `spec`, and a placeholder has no rows to
/// compare. Each body is drawn from the specification's own table, so a row
/// added there appears here and nowhere is there a second copy to disagree.
fn ready_body(card: &Card, rect: Rect, palette: Palette) -> Vec<Scene> {
    let id = card.id().as_str();
    match kind_of(id) {
        "packet" => stream_body(id, rect, palette),
        "decode" => decode_body(id, rect, palette),
        "keymap" => map_body(id, rect, palette),
        "filter" => filter_body(id, rect, palette),
        // A kind with no body painter of its own still reads as content rather
        // than as a gap. Reachable only if the catalogue grows a placeable kind
        // before its body does, which is the moment a placeholder is honest.
        other => placeholder_body(other, id, rect, palette),
    }
}

/// The ink a message type is drawn in.
///
/// Looked up by the type's position in the specification's legend, so a row
/// carrying a type the legend does not list is drawn muted rather than
/// silently taking a colour that means something else. The alternative -- a
/// match on the words -- lets the two lists drift and says nothing when they do.
const TYPE_INKS: [Color; 5] = [
    rgb(0x2D_6C_DF),
    rgb(0xC7_78_00),
    rgb(0x1F_8A_4C),
    rgb(0x8A_5C_F6),
    rgb(0x0E_9A_A7),
];

fn type_ink(word: &str, palette: Palette) -> Color {
    spec::STREAM_TYPES
        .iter()
        .position(|known| *known == word)
        .and_then(|n| TYPE_INKS.get(n).copied())
        .unwrap_or(palette.muted)
}

/// The x offset and **text width** of each stream column inside a body of that
/// width, for the columns that fit.
///
/// Derived from the specification's column table: the one column whose width is
/// `0` takes what the others leave, so a body narrower than the fixed columns
/// gives it nothing rather than wrapping into the next card.
///
/// ★ The gutter is subtracted HERE, and a column with nothing left after it is
/// dropped rather than returned at zero. The first draft returned the cell
/// width and left each caller to write `w - 6`, which underflows the moment a
/// card is one cell wide: a debug panic, and in release a column four billion
/// pixels across. A counterfactual found it -- no swept state had ever been
/// small enough -- and the repair is to make the subtraction impossible to get
/// wrong rather than to write it correctly in four places.
fn stream_columns(width: u32) -> Vec<(&'static str, u32, u32)> {
    const GUTTER: u32 = 6;
    /// The narrowest a column can be and still say anything.
    const FLOOR: u32 = 18;
    let fixed: u32 = spec::STREAM_COLUMNS.iter().map(|(_, w)| *w).sum();
    let flexible = width.saturating_sub(fixed + 24);
    let mut out = Vec::new();
    let mut x = 12;
    let mut left = width.saturating_sub(12);
    for (name, w) in spec::STREAM_COLUMNS {
        let wanted = if *w == 0 { flexible } else { *w };
        // A column takes what it wants or what is left, whichever is smaller,
        // and the ones with nothing left are dropped from the right. The label
        // elides rather than overflowing, so a narrowed column says it was cut
        // instead of painting over its neighbour.
        let cell = wanted.min(left);
        let text = cell.saturating_sub(GUTTER);
        if text < FLOOR {
            break;
        }
        out.push((*name, x, text));
        x += cell;
        left -= cell;
    }
    out
}

/// A label that says so when it does not fit, rather than painting past its
/// box. `Ellipsis` for a value read from the left, `EllipsisStart` for a path
/// whose leaf is what identifies it (R1654's distinction).
fn clipped(text: &str, rect: Rect, px: u32, fg: Color, overflow: TextOverflow) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            rect,
            TextStyle::new()
                .with_size_px(px)
                .with_fg(fg)
                .with_overflow(overflow),
        )
        .with_layout(absolute(rect)),
    )
}

/// The message stream: a header row of columns over the opening rows.
fn stream_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const HEAD_H: u32 = 20;
    const ROW_H: u32 = 20;
    let columns = stream_columns(rect.w);
    let mut out = vec![Scene::Container(
        ContainerNode::new(
            columns
                .iter()
                .map(|(name, x, w)| {
                    clipped(
                        name,
                        Rect::new(*x, 4, *w, 13),
                        FONT_TINY,
                        palette.muted,
                        TextOverflow::Ellipsis,
                    )
                })
                .collect(),
        )
        .with_tag(format!("card.{id}.head"))
        .with_style(BoxStyle::filled(palette.raised))
        .with_layout(absolute(Rect::new(rect.x, rect.y, rect.w, HEAD_H))),
    )];
    for (n, (time, kind, name, len)) in spec::STREAM_ROWS.iter().enumerate() {
        let top = rect.y + HEAD_H + u(n) * ROW_H;
        // A row whose bottom would leave the card is not painted at all. The
        // alternative -- painting it and letting it land on the card below --
        // is the defect R1656 measured on twenty-five surfaces.
        if top + ROW_H > rect.y + rect.h {
            break;
        }
        // A cell per column that fits, in the specification's order, with the
        // row's own values. Zipped rather than indexed: a narrow card drops
        // columns from the right, and indexing would reach past the end.
        let values = [*time, *kind, *name, *len];
        let cells = columns
            .iter()
            .zip(values)
            .map(|((column, x, w), value)| {
                let ink = if *column == "type" {
                    type_ink(value, palette)
                } else if *column == "name" {
                    palette.ink
                } else {
                    palette.muted
                };
                // A resource path is identified by its leaf, so it gives up
                // its head; everything else reads from the left.
                let overflow = if *column == "name" {
                    TextOverflow::EllipsisStart
                } else {
                    TextOverflow::Ellipsis
                };
                clipped(value, Rect::new(*x, 3, *w, 13), FONT_TINY, ink, overflow)
            })
            .collect();
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.row.{n}"))
                .with_layout(absolute(Rect::new(rect.x, top, rect.w, ROW_H))),
        ));
    }
    out
}

/// The decode inspector: the layer tree beside the bytes it decoded.
fn decode_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const ROW_H: u32 = 19;
    // The tree keeps at least half the card; the bytes pane takes what is left,
    // and is dropped entirely when that is less than one byte's worth. A fixed
    // pane on a card narrower than the pane paints outside the card, which the
    // gate reports and a reader sees as one card's bytes on the next one.
    let bytes_w = BYTES_W.min(rect.w / 2);
    let tree_w = if bytes_w >= BYTES_FLOOR {
        rect.w.saturating_sub(bytes_w + 12)
    } else {
        rect.w
    };
    let mut out = Vec::new();
    for (n, (depth, key, value)) in spec::DECODE_ROWS.iter().enumerate() {
        let top = rect.y + 4 + u(n) * ROW_H;
        if top + ROW_H > rect.y + rect.h {
            break;
        }
        let indent = (10 + depth * 12).min(tree_w);
        let heading = *depth == 0;
        let selected = n == spec::DECODE_SELECTED;
        // ★ The key is the identifying half, so it is allocated FIRST and the
        // value gets what is left. The first draft gave the value a fixed
        // seventy-four pixels at a fixed offset and let the key take the
        // remainder: on a narrowed card the remainder was nothing, the key
        // vanished, and the row read as a value with no name. A positional
        // comparison against the specification is what caught it -- a check
        // that asked only "are these the row's words" saw the same set.
        let room = tree_w.saturating_sub(indent + 6);
        let with_value = !value.is_empty() && room >= KEY_FLOOR + VALUE_W;
        let key_w = if with_value { room - VALUE_W } else { room };
        let mut cells = Vec::new();
        if key_w > 0 {
            cells.push(clipped(
                key,
                Rect::new(indent, 3, key_w, 13),
                FONT_TINY,
                if heading { palette.ink } else { palette.muted },
                TextOverflow::Ellipsis,
            ));
        }
        if with_value {
            cells.push(clipped(
                value,
                Rect::new(indent + key_w, 3, VALUE_W, 13),
                FONT_TINY,
                palette.ink,
                TextOverflow::EllipsisStart,
            ));
        }
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.tree.{n}"))
                .with_style(if selected {
                    BoxStyle::filled(palette.high).with_corner_radius(4)
                } else {
                    BoxStyle::default()
                })
                .with_layout(absolute(Rect::new(rect.x, top, tree_w, ROW_H))),
        ));
    }
    out.extend(byte_pane(
        id,
        Rect::new(rect.x + tree_w + 12, rect.y, bytes_w, rect.h),
        rect,
        palette,
    ));
    out
}

/// The bytes the decode card shows beside its tree, four per line, with the
/// selected field's own bytes lit.
///
/// The law screen B is built on (R1663): what is drawn lit is exactly what the
/// map says the selection occupies, not a resemblance of it.
fn byte_pane(id: &str, pane: Rect, card: Rect, palette: Palette) -> Vec<Scene> {
    const ROW_H: u32 = 19;
    let mut out = Vec::new();
    if pane.w < BYTES_FLOOR {
        return out;
    }
    let (start, end) = spec::DECODE_SELECTED_SPAN;
    for (line, quad) in spec::DECODE_BYTES.iter().enumerate() {
        let top = card.y + 4 + u(line) * ROW_H;
        if top + ROW_H > card.y + card.h {
            break;
        }
        let mut cells = vec![label(
            &format!("{:04x}", line * 4),
            Rect::new(6, 3, 30, 13),
            FONT_TINY,
            palette.muted,
        )];
        for (col, byte) in quad.iter().enumerate() {
            let index = line * 4 + col;
            // A cell that would leave the pane is not painted. Its bytes are
            // not lost -- the pane is simply narrower than four columns, and a
            // cell drawn past the edge lands on whatever is beside the card.
            if 40 + u(col) * 24 + 22 > pane.w {
                break;
            }
            let lit = index >= start && index < end;
            cells.push(Scene::Container(
                ContainerNode::new(vec![label(
                    &format!("{byte:02x}"),
                    Rect::new(2, 3, 18, 13),
                    FONT_TINY,
                    if lit { palette.on_accent } else { palette.ink },
                )])
                .with_tag(format!("card.{id}.byte.{index}"))
                .with_style(if lit {
                    BoxStyle::filled(palette.accent).with_corner_radius(3)
                } else {
                    BoxStyle::default()
                })
                .with_layout(absolute(Rect::new(40 + u(col) * 24, 0, 22, ROW_H))),
            ));
        }
        out.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("card.{id}.bytes.{line}"))
                .with_layout(absolute(Rect::new(pane.x, top, pane.w, ROW_H))),
        ));
    }
    out
}

/// The identifier map: numeric id to resource path, and when it was declared.
fn map_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    const HEAD_H: u32 = 18;
    const ROW_H: u32 = 18;
    // The columns are allocated left to right and a column with nothing left is
    // dropped, so a narrowed card shows the id and the resource rather than the
    // id and the timestamp. Same discipline as the stream's columns and the
    // decode tree's key: the identifying half is allocated first.
    const ID_W: u32 = 34;
    const SEEN_W: u32 = 66;
    const PATH_FLOOR: u32 = 40;
    let room = rect.w.saturating_sub(12 + ID_W + 6);
    let with_seen = room >= PATH_FLOOR + SEEN_W;
    let path_w = if with_seen { room - SEEN_W } else { room };
    let cells = |ink: Color, cols: [&str; 3], warn: bool| {
        let mut out = vec![clipped(
            cols[0],
            Rect::new(12, 2, ID_W, 13),
            FONT_TINY,
            if warn { palette.warn } else { ink },
            TextOverflow::Ellipsis,
        )];
        if path_w > 0 {
            out.push(clipped(
                cols[1],
                Rect::new(12 + ID_W + 6, 2, path_w, 13),
                FONT_TINY,
                ink,
                TextOverflow::EllipsisStart,
            ));
        }
        if with_seen {
            out.push(clipped(
                cols[2],
                Rect::new(12 + ID_W + 6 + path_w, 2, SEEN_W, 13),
                FONT_TINY,
                palette.muted,
                TextOverflow::Ellipsis,
            ));
        }
        out
    };
    let mut out = vec![Scene::Container(
        ContainerNode::new(cells(
            palette.muted,
            [
                spec::MAP_COLUMNS[0],
                spec::MAP_COLUMNS[1],
                spec::MAP_COLUMNS[2],
            ],
            false,
        ))
        .with_tag(format!("card.{id}.head"))
        .with_style(BoxStyle::filled(palette.raised))
        .with_layout(absolute(Rect::new(rect.x, rect.y, rect.w, HEAD_H))),
    )];
    for (n, (key, path, seen)) in spec::MAP_ROWS.iter().enumerate() {
        let top = rect.y + HEAD_H + u(n) * ROW_H;
        if top + ROW_H > rect.y + rect.h {
            break;
        }
        let unresolved = n == spec::MAP_UNRESOLVED;
        out.push(Scene::Container(
            ContainerNode::new(cells(
                if unresolved {
                    palette.warn
                } else {
                    palette.ink
                },
                [key, path, seen],
                unresolved,
            ))
            .with_tag(format!("card.{id}.map.{n}"))
            .with_layout(absolute(Rect::new(rect.x, top, rect.w, ROW_H))),
        ));
    }
    out
}

/// The search and filter card: the query, the saved chips, and the three counts
/// whose relation is the point of the card.
fn filter_body(id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let mut out = vec![Scene::Container(
        ContainerNode::new(vec![label(
            spec::FILTER_QUERY,
            Rect::new(10, 7, rect.w.saturating_sub(36), 14),
            FONT_SMALL,
            palette.ink,
        )])
        .with_tag(format!("card.{id}.query"))
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(7)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(Rect::new(rect.x, rect.y, rect.w, 28))),
    )];
    let mut x = rect.x;
    let mut y = rect.y + 34;
    for (n, (name, on)) in spec::FILTER_CHIPS.iter().enumerate() {
        // A chip is as wide as its word, and a row of them wraps rather than
        // running off the card -- the policy R1651 measured on the reference's
        // own form and lifted into the framework. ★ It is also CLAMPED to the
        // card: a word longer than the card cannot be made to fit by wrapping,
        // and the first draft let it run off the right edge on a one-cell card.
        let w = (18 + u(name.chars().count()) * 6).min(rect.w);
        if x + w > rect.x + rect.w {
            x = rect.x;
            y += 26;
        }
        if y + 22 > rect.y + rect.h {
            break;
        }
        out.push(Scene::Container(
            ContainerNode::new(vec![clipped(
                name,
                Rect::new(9, 4, w.saturating_sub(18), 13),
                FONT_TINY,
                if *on {
                    palette.on_accent
                } else {
                    palette.muted
                },
                TextOverflow::Ellipsis,
            )])
            .with_tag(format!("card.{id}.chip.{n}"))
            .with_style(
                BoxStyle::filled(if *on { palette.accent } else { palette.raised })
                    .with_corner_radius(10)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(x, y, w, 22))),
        ));
        x += w + 6;
    }
    out.extend(filter_counts(
        id,
        Rect::new(
            rect.x,
            y + 30,
            rect.w,
            rect.y + rect.h - (y + 30).min(rect.y + rect.h),
        ),
        rect,
        palette,
    ));
    out
}

/// The filter card's three counts, and the recent past of the first.
///
/// Three rather than one because the reference's point is the RELATION -- a
/// reader is looking at a subset of a subset, and a single number cannot say
/// which subset it is. The tiles go or stay together: a card too short for them
/// shows the query and the chips, which are the parts a reader can still act on.
fn filter_counts(id: &str, area: Rect, card: Rect, palette: Palette) -> Vec<Scene> {
    let mut out = Vec::new();
    let stat_w = area.w.saturating_sub(2 * 8) / u(spec::FILTER_STATS.len());
    if stat_w < STAT_FLOOR || area.y + STAT_H > card.y + card.h {
        return out;
    }
    for (n, (value, what)) in spec::FILTER_STATS.iter().enumerate() {
        out.push(Scene::Container(
            ContainerNode::new(vec![
                label(
                    value,
                    Rect::new(10, 7, stat_w.saturating_sub(20), 17),
                    FONT_TITLE,
                    palette.ink,
                ),
                label(
                    what,
                    Rect::new(10, 27, stat_w.saturating_sub(20), 13),
                    FONT_TINY,
                    palette.muted,
                ),
            ])
            .with_tag(format!("card.{id}.stat.{n}"))
            .with_style(
                BoxStyle::filled(palette.raised)
                    .with_corner_radius(8)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                area.x + u(n) * (stat_w + 8),
                area.y,
                stat_w,
                STAT_H,
            ))),
        ));
    }
    // The recent past of the first count, so a reader can see whether the
    // matched share is moving before reading three numbers off the tiles.
    let spark_y = area.y + 52;
    if spark_y + 30 <= card.y + card.h {
        out.push(Scene::Container(
            ContainerNode::new(vec![
                Sparkline::new(MATCH_SERIES.to_vec())
                    .with_color(kind_color("filter"))
                    .with_tag_prefix("match.spark")
                    .build(
                        Rect::new(0, 0, area.w, card.y + card.h - spark_y),
                        &ChartStyle::default(),
                    ),
            ])
            .with_tag(format!("card.{id}.sparkline"))
            .with_layout(absolute(Rect::new(
                area.x,
                spark_y,
                area.w,
                card.y + card.h - spark_y,
            ))),
        ));
    }
    out
}

/// A kind with no body painter: its code and its one line, which is what the
/// palette already told the person who placed it.
fn placeholder_body(kind: &str, id: &str, rect: Rect, palette: Palette) -> Vec<Scene> {
    let def = def_of(kind);
    vec![
        Scene::Container(
            ContainerNode::new(vec![label(
                def.map_or("", |d| d.code),
                Rect::new(9, 9, 34, 14),
                FONT_TINY,
                palette.on_accent,
            )])
            .with_tag(format!("card.{id}.code"))
            .with_style(BoxStyle::filled(kind_color(kind)).with_corner_radius(6))
            .with_layout(absolute(Rect::new(rect.x + 12, rect.y + 10, 40, 32))),
        ),
        label(
            def.map_or("", |d| d.gist),
            Rect::new(rect.x + 62, rect.y + 18, rect.w.saturating_sub(74), 16),
            FONT_BODY,
            palette.muted,
        ),
    ]
}

fn state_sentence(state: &CardState) -> String {
    match state {
        CardState::Ready => "showing content".to_string(),
        CardState::Loading => "loading\u{2026}".to_string(),
        CardState::Empty => "nothing matched this filter".to_string(),
        CardState::Failed(why) => format!("could not load: {why}"),
        CardState::Denied(what) => format!("not permitted: {what}"),
        CardState::Opaque => "link is encrypted; content unavailable".to_string(),
    }
}

const fn remedy_label(remedy: Remedy) -> &'static str {
    match remedy {
        Remedy::Wait => "waiting",
        Remedy::Retry => "Retry",
        Remedy::Widen => "Widen filter",
        Remedy::Authorize => "Request access",
        Remedy::Nothing => "nothing can be done",
    }
}

/// The size-stepper strip layout-edit mode puts on every card.
fn edit_bar_scene(card_id: &str, bar: Rect, cell: (u32, u32), palette: Palette) -> Scene {
    let mut children = Vec::new();
    for (n, (verb, glyph)) in STEPPERS.iter().enumerate() {
        let slot = stepper_rect(bar, u(n));
        children.push(Scene::Container(
            ContainerNode::new(vec![label(
                glyph,
                Rect::new(6, 2, 14, 14),
                FONT_BODY,
                palette.ink,
            )])
            .with_tag(format!("card.{card_id}.{verb}"))
            .with_style(
                BoxStyle::filled(palette.high)
                    .with_corner_radius(4)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                slot.x - bar.x,
                slot.y - bar.y,
                slot.w,
                slot.h,
            ))),
        ));
    }
    children.push(label(
        "W",
        Rect::new(58, 6, 12, 14),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        "H",
        Rect::new(136, 6, 12, 14),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        &format!("{}\u{00D7}{}", cell.0, cell.1),
        Rect::new(bar.w.saturating_sub(48), 6, 40, 14),
        FONT_SMALL,
        palette.accent_fg,
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{card_id}.editbar"))
            .with_style(BoxStyle::filled(palette.raised))
            .with_layout(absolute(bar)),
    )
}

fn card_scene(
    card: &Card,
    rect: Rect,
    selected: bool,
    editing: bool,
    cell: (u32, u32),
    palette: Palette,
) -> Scene {
    let inside = local(rect);
    let mut children = header_scene(card, header_rect(inside), palette);
    children.extend(body_scene(card, body_rect(inside, editing), palette));
    if editing {
        children.push(edit_bar_scene(
            card.id().as_str(),
            edit_bar_rect(inside),
            cell,
            palette,
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{}", card.id().as_str()))
            .with_style(
                BoxStyle::filled(palette.panel)
                    .with_corner_radius(10)
                    // The selection ring: one card is the keyboard's subject
                    // and a person has to see which. Accent on the border
                    // rather than a different fill, so a selected card that is
                    // also failing still reads as failing.
                    .with_border(if selected {
                        Border::new(palette.accent_fg, 2)
                    } else {
                        Border::new(palette.outline, 1)
                    }),
            )
            .with_layout(absolute(rect)),
    )
}

/// A detached panel, floating over the canvas.
fn float_scene(state: &ShellState, float: &Float, palette: Palette) -> Option<Scene> {
    let card = state.card(&float.id)?;
    let rect = float_rect(float);
    let inside = local(rect);
    let header = header_rect(inside);
    let colour = kind_color(kind_of(&float.id));
    let mut children = vec![
        dot(14, header.y + CARD_HDR / 2 - 4, 9, colour),
        label(
            card.title(),
            Rect::new(30, header.y + 9, header.w.saturating_sub(200), 16),
            FONT_BODY,
            palette.ink,
        ),
        // The badge that says this panel is not on the board.
        Scene::Container(
            ContainerNode::new(vec![label(
                "DETACHED",
                Rect::new(9, 4, 66, 12),
                FONT_TINY,
                palette.muted,
            )])
            .with_tag(format!("float.{}.badge", float.id))
            .with_style(
                BoxStyle::filled(palette.raised)
                    .with_corner_radius(4)
                    .with_border(Border::new(palette.outline, 1)),
            )
            .with_layout(absolute(Rect::new(
                header.w.saturating_sub(160),
                header.y + 8,
                84,
                20,
            ))),
        ),
        Scene::Container(
            ContainerNode::new(vec![redock_mark(
                local(affordance_rect(header, 2, 0)),
                palette.muted,
            )])
            .with_tag(format!("float.{}.redock", float.id))
            .with_layout(absolute(affordance_rect(header, 2, 0))),
        ),
        Scene::Container(
            ContainerNode::new(vec![close_mark(
                local(affordance_rect(header, 2, 1)),
                palette.muted,
            )])
            .with_tag(format!("float.{}.close", float.id))
            .with_layout(absolute(affordance_rect(header, 2, 1))),
        ),
    ];
    children.extend(body_scene(&card, body_rect(inside, false), palette));
    Some(Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("float.{}", float.id))
            .with_style(
                BoxStyle::filled(palette.panel)
                    .with_corner_radius(10)
                    .with_border(Border::new(palette.accent_fg, 1)),
            )
            .with_layout(absolute(rect)),
    ))
}

/// R1668 — the reads that answer from the SPECIFICATION rather than from the
/// shell's state.
///
/// Grouped because that is the distinction: nothing here can change while the
/// application runs, so a client that caches one of these is right to.
fn read_specification(path: &str) -> Result<IntrospectValue, ReadRefusal> {
    match path {
        "spec" => Ok(IntrospectValue::Json(spec_json())),
        "rail" => Ok(IntrospectValue::Text(
            spec::RAIL
                .iter()
                .map(|seat| seat.key)
                .collect::<Vec<_>>()
                .join(","),
        )),
        // The seats a later release opens, and what each is booked under. A
        // locked seat that could only be SEEN is a screenshot; one that says
        // what it is waiting for is a specification.
        "reserved_rail" => Ok(IntrospectValue::Json(serde_json::Value::Array(
            spec::RAIL
                .iter()
                .filter_map(|seat| {
                    seat.reserved_for
                        .map(|why| serde_json::json!({ "key": seat.key, "reserved_for": why }))
                })
                .collect(),
        ))),
        // The thirteen kinds, so a client picks from what is offered.
        "catalogue" => Ok(IntrospectValue::Text(
            spec::CATALOGUE
                .iter()
                .map(|w| w.kind)
                .collect::<Vec<_>>()
                .join(","),
        )),
        // Unreachable through the caller's match, which routes exactly the
        // four paths above. Stated rather than unwrapped: a fifth path added to
        // that arm and not to this one refuses in the schema's own vocabulary
        // instead of panicking a shell.
        _ => Err(ReadRefusal::UnknownPath),
    }
}

/// R1668 — the reference screen C, as the wire hands it to a client.
///
/// Every field is read straight out of `spec`, so the running application and
/// the gate compare against one table rather than two spellings of it.
fn spec_json() -> serde_json::Value {
    serde_json::json!({
        "window": { "w": spec::WIN_W, "h": spec::WIN_H },
        "metrics": {
            "app_bar_h": spec::APP_BAR_H,
            "sub_bar_h": spec::SUB_BAR_H,
            "rail_w": spec::RAIL_W,
            "palette_w": spec::PALETTE_W,
            "grid_cols": spec::GRID_COLS,
        },
        "source": spec::SOURCE,
        "transport": spec::TRANSPORT,
        "rate": spec::RATE,
        "preset": spec::PRESET,
        "board_verbs": spec::BOARD_VERBS,
        "rail": spec::RAIL.iter().map(|seat| serde_json::json!({
            "key": seat.key, "title": seat.title, "reserved_for": seat.reserved_for,
        })).collect::<Vec<_>>(),
        "rail_active": spec::RAIL_ACTIVE,
        "sections": spec::SECTIONS.iter().map(|(key, title, tier)| serde_json::json!({
            "key": key, "title": title, "tier": tier_word(*tier),
        })).collect::<Vec<_>>(),
        "catalogue": spec::CATALOGUE.iter().map(|w| serde_json::json!({
            "kind": w.kind,
            "code": w.code,
            "label": w.label,
            "gist": w.gist,
            "section": w.section,
            "tier": tier_word(w.tier),
            "reserved_for": w.reserved_for,
        })).collect::<Vec<_>>(),
        "placeable_count": spec::placeable_count(),
        "reserved_count": spec::reserved_count(),
        "board": spec::BOARD.iter().map(|p| serde_json::json!({
            "kind": p.kind, "col": p.col, "row": p.row, "cols": p.cols, "rows": p.rows,
        })).collect::<Vec<_>>(),
        "card_chrome": spec::CARD_CHROME,
        "stream_columns": spec::STREAM_COLUMNS.iter().map(|(name, w)| serde_json::json!({
            "name": name, "width": w,
        })).collect::<Vec<_>>(),
        "stream_rows": spec::STREAM_ROWS.iter().map(|(time, kind, name, len)| serde_json::json!({
            "time": time, "type": kind, "name": name, "len": len,
        })).collect::<Vec<_>>(),
        "decode_rows": spec::DECODE_ROWS.iter().map(|(depth, key, value)| serde_json::json!({
            "depth": depth, "key": key, "value": value,
        })).collect::<Vec<_>>(),
        "decode_selected": spec::DECODE_SELECTED,
        "decode_span": [spec::DECODE_SELECTED_SPAN.0, spec::DECODE_SELECTED_SPAN.1],
        "map_rows": spec::MAP_ROWS.iter().map(|(id, path, seen)| serde_json::json!({
            "id": id, "resource": path, "first_seen": seen,
        })).collect::<Vec<_>>(),
        "map_unresolved": spec::MAP_UNRESOLVED,
        "filter_query": spec::FILTER_QUERY,
        "filter_chips": spec::FILTER_CHIPS.iter().map(|(name, on)| serde_json::json!({
            "name": name, "on": on,
        })).collect::<Vec<_>>(),
        "filter_stats": spec::FILTER_STATS.iter().map(|(value, what)| serde_json::json!({
            "value": value, "of": what,
        })).collect::<Vec<_>>(),
    })
}

/// The wire spelling of a tier.
const fn tier_word(tier: spec::Tier) -> &'static str {
    match tier {
        spec::Tier::Placeable => "placeable",
        spec::Tier::Reserved => "reserved",
    }
}

/// One palette row: the swatch, the name, the one line, and what the row offers
/// -- the verb for a placeable entry, the booking for a reserved one.
fn palette_row(
    state: &ShellState,
    def: &'static spec::WidgetSpec,
    rect: Rect,
    palette: Palette,
) -> Scene {
    let placed = state
        .cards
        .get()
        .iter()
        .filter(|c| kind_of(c.id().as_str()) == def.kind)
        .count();
    let reserved = def.tier == spec::Tier::Reserved;
    // The reference shows the reservation in the slot the add control would
    // occupy, so a reader's eye finds "what does this row offer me" in one
    // place down the whole list.
    //
    // ★ Its width is what the text beside it gives up. The first draft sized
    // the text against the ADD control and then put a wider word in the slot,
    // and every one of the nine reserved rows painted its name under the
    // badge -- eighteen overlapping pairs, which the gate reported before
    // anybody looked at the window.
    let (trailing_w, trailing) = if reserved {
        (
            52,
            label(
                "later",
                Rect::new(rect.w.saturating_sub(52), 15, 46, 16),
                FONT_SMALL,
                palette.muted,
            ),
        )
    } else {
        (
            34,
            label(
                &if placed == 0 {
                    "+".to_string()
                } else {
                    format!("+ {placed}")
                },
                Rect::new(rect.w.saturating_sub(30), 15, 26, 16),
                FONT_BODY,
                palette.accent_fg,
            ),
        )
    };
    let text_w = rect.w.saturating_sub(50 + trailing_w + 6);
    // R1668 — a reserved row is DECLARED unavailable and states what it is
    // booked under. Everything a reader and an agent get from that -- inert to
    // the pointer, faded ink, `aria-disabled` with a spoken reason, a row on
    // `scene/disabled` naming the requirement -- follows from this one
    // declaration rather than from four hand-kept copies of it.
    let layout = if reserved {
        absolute(rect).with_unavailable(Unavailable::reserved(def.reserved_for))
    } else {
        absolute(rect)
    };
    Scene::Container(
        ContainerNode::new(vec![
            Scene::Container(
                ContainerNode::new(vec![label(
                    def.code,
                    Rect::new(5, 9, 30, 14),
                    FONT_TINY,
                    palette.on_accent,
                )])
                .with_style(BoxStyle::filled(kind_color(def.kind)).with_corner_radius(8))
                .with_layout(absolute(Rect::new(8, 7, 32, 32))),
            ),
            // Both elide. A palette is a list of names of varying length in a
            // fixed column, so "the longest one happens to fit" is not a
            // property anybody can keep -- and the boot gate measured this one
            // at ten pixels outside its row.
            clipped(
                def.label,
                Rect::new(50, 8, text_w, 16),
                FONT_BODY,
                palette.ink,
                TextOverflow::Ellipsis,
            ),
            clipped(
                def.gist,
                Rect::new(50, 26, text_w, 14),
                FONT_SMALL,
                palette.muted,
                TextOverflow::Ellipsis,
            ),
            trailing,
        ])
        .with_tag(format!("shell.palette.{}", def.kind))
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(10)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(layout),
    )
}

/// The palette panel: the catalogue, grouped, with a count at the foot.
fn palette_scene(state: &ShellState, palette: Palette) -> Scene {
    let panel = palette_rect();
    let mut children = vec![
        label(
            spec::PALETTE_TITLE,
            Rect::new(16, 18, 220, 20),
            FONT_TITLE,
            palette.ink,
        ),
        label(
            spec::PALETTE_HINT,
            Rect::new(16, 42, 250, 16),
            FONT_SMALL,
            palette.muted,
        ),
    ];
    for (def, title, rect) in palette_rows() {
        match def {
            None => children.push(label(title, rect, FONT_TINY, palette.muted)),
            Some(def) => children.push(palette_row(state, def, rect, palette)),
        }
    }
    // Both counts, because the screen's whole claim is the relation between
    // them: this release places four, and holds nine seats open.
    children.push(label(
        &format!(
            "{} placed of {}",
            state.placed().len(),
            spec::placeable_count()
        ),
        Rect::new(16, panel.h.saturating_sub(30), 130, 16),
        FONT_SMALL,
        palette.muted,
    ));
    children.push(label(
        &format!("{} reserved", spec::reserved_count()),
        Rect::new(
            panel.w.saturating_sub(110),
            panel.h.saturating_sub(30),
            94,
            16,
        ),
        FONT_SMALL,
        palette.muted,
    ));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag("shell.palette")
            .with_style(BoxStyle::filled(palette.panel))
            .with_layout(absolute(panel)),
    )
}

/// The toast: what just happened, floating at the foot of the canvas.
fn toast_scene(state: &ShellState, palette: Palette) -> Scene {
    let canvas = canvas_rect();
    let rect = Rect::new(canvas.x + 24, WIN_H - 58, 560, 34);
    Scene::Container(
        ContainerNode::new(vec![
            dot(14, 13, 8, palette.accent_fg),
            label(
                &state.toast.get(),
                Rect::new(32, 9, rect.w.saturating_sub(44), 16),
                FONT_BODY,
                palette.ink,
            ),
        ])
        .with_tag("shell.toast")
        .with_style(
            BoxStyle::filled(palette.raised)
                .with_corner_radius(10)
                .with_border(Border::new(palette.outline, 1)),
        )
        .with_layout(absolute(rect)),
    )
}

fn view(_state: (), _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_shell_state();
    let dark = theme_word(&state.theme) == "dark";
    let palette = Palette {
        ink: theme.resolve(ColorRole::OnSurface),
        muted: theme.resolve(ColorRole::OnSurfaceMuted),
        accent: theme.resolve(ColorRole::Accent),
        on_accent: theme.resolve(ColorRole::OnAccent),
        accent_fg: theme.resolve(ColorRole::InversePrimary),
        canvas: theme.resolve(ColorRole::Surface),
        panel: theme.resolve(ColorRole::SurfaceContainerLow),
        raised: theme.resolve(ColorRole::SurfaceContainer),
        high: theme.resolve(ColorRole::SurfaceContainerHigh),
        outline: theme.resolve(ColorRole::Outline),
        grid: grid_ink(dark),
        warn: theme.resolve(ColorRole::Warning),
    };

    let board = state.board.get();
    let selected = state.selected.get();
    let editing = state.editing.get();
    let drag = state.drag.get();

    let mut canvas_children = grid_scene(board.rows() + 1, palette, editing || drag.is_some());
    for card in &state.placed() {
        let Some(tile) = board.tile(card.id()) else {
            continue;
        };
        canvas_children.push(card_scene(
            card,
            cell_rect(tile),
            selected.as_deref() == Some(card.id().as_str()),
            editing,
            (tile.w, tile.h),
            palette,
        ));
    }
    // ★ The snap preview: where a release would put the dragged card. Drawn
    // rather than moving the card, because the reference commits on release and
    // a board reflowing under the finger would make the preview a lie.
    if let Some(drag) = &drag
        && let Some(tile) = board.tile(&drag.id)
    {
        let ghost = Tile::new(drag.id.as_str(), drag.snap.0, drag.snap.1, tile.w, tile.h);
        canvas_children.push(Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag("shell.dropslot")
                .with_style(
                    BoxStyle::filled(palette.high)
                        .with_corner_radius(10)
                        .with_border(Border::new(palette.accent_fg, 2)),
                )
                .with_layout(absolute(cell_rect(&ghost))),
        ));
    }
    // ★ R1662 — the BOARD scrolls and the floats do not. A torn-off card is
    // chrome over the canvas, so it keeps its place when the board slides;
    // that is the same split the node lab makes between its world surface and
    // the gate panel floating over it. The scroll range is derived from the
    // cards themselves by the pane, so a board that grows a row cannot outrun
    // a number written here.
    let mut canvas_children = vec![scroll_pane(
        &state.canvas_scroll,
        Rect::new(0, 0, canvas_rect().w, canvas_rect().h),
        (0, GAP),
        // Every press goes to the one root `External` that runs this screen's
        // own hit test, so the pane must be invisible to the router (R1655).
        PanePointer::PassesThrough,
        canvas_children,
    )];
    for float in &state.floats.get() {
        if let Some(scene) = float_scene(&state, float, palette) {
            canvas_children.push(scene);
        }
    }

    let children = vec![
        Scene::Container(
            ContainerNode::new(canvas_children)
                .with_tag("shell.canvas")
                .with_style(BoxStyle::filled(palette.canvas))
                .with_layout(absolute(canvas_rect())),
        ),
        app_bar_scene(&state, palette),
        sub_bar_scene(&state, palette),
        rail_scene(&state, palette),
        palette_scene(&state, palette),
        toast_scene(&state, palette),
        label(
            HELP_STRIP,
            Rect::new(canvas_rect().x + 610, WIN_H - 47, 470, 14),
            FONT_SMALL,
            palette.muted,
        ),
    ];

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(palette.canvas))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct AnalyzerShellView;

impl WidgetCore for AnalyzerShellView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = ShellOracle::new();
        oracle.attach_state(use_shell_state());
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
        "pinion hello-analyzer-shell (R1649 §5.21 analysis-tool dashboard shell)"
    }
}

impl WidgetA11y for AnalyzerShellView {
    /// The board is a group, and **every card is a node that says what it is
    /// showing**.
    ///
    /// The half a paint cannot carry, and the reason the state is a value: a
    /// card that failed announces its failure and its remedy. Measured on the
    /// toolkit at 6.11, no panel or view class has a content-state concept, so
    /// this is not something an assistive technology can be told there.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_shell_state();
        let mut nodes = vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Analyzer dashboard")
                .with_value(AccessValue::Text(format!(
                    "{} of {} widgets placed on layout \"{}\", {} reserved, source {}",
                    state.placed().len(),
                    spec::placeable_count(),
                    state.preset.get(),
                    spec::reserved_count(),
                    state.source.get(),
                ))),
        ];
        for card in &state.cards.get() {
            let id = card.id().as_str();
            let where_it_is = if state.is_floating(id) {
                "detached; "
            } else {
                ""
            };
            let announce = match card.remedy() {
                None => format!("{where_it_is}{}", state_sentence(card.state())),
                Some(remedy) => format!(
                    "{where_it_is}{}; {}",
                    state_sentence(card.state()),
                    remedy_label(remedy)
                ),
            };
            nodes.push(
                AccessNode::new(format!("card.{id}"), AriaRole::Group)
                    .with_name(card.title())
                    .with_value(AccessValue::Text(announce))
                    .with_state(AccessState::default()),
            );
        }
        nodes
    }
}

impl WidgetView for AnalyzerShellView {
    type Renderer = HelloAnalyzerShellRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<AnalyzerShellView>();
}

#[cfg(test)]
mod painted;
#[cfg(test)]
mod tests;
