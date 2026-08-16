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

use pinion_a11y::{
    AccessFocus, AccessLive, AccessNode, AccessState, AccessValue, AriaRole, GridCell, GridColumn,
    GridRow, HasPopup, NavLink, WidgetA11y, grid_table_nodes, navigation_link_nodes,
    page_region_node,
};
use pinion_chart::{ChartStyle, Sparkline};
use pinion_core::availability::Unavailable;
use pinion_core::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, PointerTarget, ReadRefusal,
    RepaintOwner, SchemaArg, SchemaField, ThreadOwnership,
};
use pinion_core::focus_state;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    Border, BoxStyle, Chrome, ChromeEdge, ChromeRole, Color, LayoutStyle, PathStyle, Size, Stroke,
    TextOverflow, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, ThemeMode, ThemeProvider, use_theme};
use pinion_core::voice::Silence;
use pinion_core::widgets::button::ButtonState;
use pinion_core::widgets::card::{Card, CardAffordance, CardChrome, CardState, Remedy};
use pinion_core::widgets::destination::{Destinations, Detour, Journey};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::roving::{Activation, Axis, Ends, Landing, Member, Roving, RovingSpec};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::tile_grid::{
    Maximized, Tile, TileDirection, TileGrid, TileId, TileNudge,
};
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::transport::{TransportClock, TransportStatus, use_transport_clock};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{self, ButtonColors, ButtonStyle};
use pinion_widget_paint::pages::view_page_region;
use pinion_widget_paint::pane::{PanePointer, scroll_pane};
use pinion_widget_paint::run::text_run;
use pinion_widget_paint::switch::{self, SwitchStyle};

mod spec;

// pinion-forge codegen output: `pub struct HelloAnalyzerShellRenderer` + its
// error type + async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloAnalyzerShellRenderer, HelloAnalyzerShellRendererError);

/// The size the specification's rectangles were measured against, and the
/// floor this screen declares to the shell.
const WIN_W: u32 = spec::WIN_W;
const WIN_H: u32 = spec::WIN_H;

/// The live surface, or the design size where no shell has published one.
///
/// ★ R1671 — reported by a person maximising the window: the content stayed
/// where it was and the rest of the window was empty. Every rectangle on this
/// screen was authored against the CONSTANTS, so the screen painted 1440x900
/// in whatever window it was given. Screens A and B have read the live size
/// since R1654; this one never did, and its own painted sweep could not see it
/// because every check compared the screen to itself.
///
/// `use_viewport_size` is a tracked read, so the view re-runs on a resize; it
/// is strict about the owner scope and a bare unit call has none. The design
/// size is the honest fallback there — it is what the specification measured.
/// Below the floor it is also the answer: the shell declares `SizeStrategy::
/// Fixed`, so a smaller surface is not a state this screen can be dragged into.
fn window_size() -> (u32, u32) {
    // ★★★★★ R1700 — the framework's policy, not this screen's copy of it.
    //
    // R1671 fixed the two-sizes defect here by putting the size in this
    // screen's own state and reading it through a weak handle off a view scope.
    // That was correct and it was a SECOND spelling: the node lab read the
    // framework's record instead, and the capture viewer had a third that was
    // simply wrong. Three versions of one policy, one of them defective, is
    // what `layout_size` exists to end — and it also removes the reason
    // `ShellState::surface` had to be read from two routes.
    pinion_core::external::layout_size(VIEW_TAG, (WIN_W, WIN_H), (WIN_W, WIN_H))
}

/// The live surface width, and height.
fn win_w() -> u32 {
    window_size().0
}

fn win_h() -> u32 {
    window_size().1
}

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
/// R1697 — the floor a resize clamps at, from the reference's own source.
///
/// Not a guess and not a fraction of the opening size: the reference writes
/// `Math.max(320, …)` and `Math.max(220, …)` literally, and a panel smaller
/// than that cannot show its header controls beside its title.
const FLOAT_MIN_W: u32 = 320;
const FLOAT_MIN_H: u32 = 220;
/// The corner a resize is grabbed by, square, inside the panel's bottom right.
const FLOAT_GRIP: u32 = 16;

/// R1662 — the input-router tag the board's scrolling body answers to.
const CANVAS_SCROLL: &str = "shell.canvas.body";

const FONT_TITLE: u32 = 13;
const FONT_BODY: u32 = 12;
const FONT_SMALL: u32 = 11;
const FONT_TINY: u32 = 10;

/// The canvas rectangle: everything between the rail and the palette, under
/// both bars.
fn canvas_rect() -> Rect {
    Rect::new(
        RAIL_W,
        APP_BAR_H + SUB_BAR_H,
        win_w() - RAIL_W - PALETTE_W,
        win_h() - APP_BAR_H - SUB_BAR_H,
    )
}

/// ★★ R1695 — the rectangle the **paged region** occupies at a destination.
///
/// At the dashboard it is exactly [`canvas_rect`], because the dashboard also
/// paints a layout bar above it and a palette beside it; anywhere else those
/// are not there and the page gets the whole area the rail and application bar
/// leave. A destination-dependent rectangle is what a region is: the page is
/// what the window gives that destination, not a fixed hole in the chrome.
fn page_rect(at: &str) -> Rect {
    if at == "dashboard" {
        return canvas_rect();
    }
    Rect::new(RAIL_W, APP_BAR_H, win_w() - RAIL_W, win_h() - APP_BAR_H)
}

/// The opening value of each Settings switch, from the specification.
fn opening_options() -> [bool; spec::OPTIONS.len()] {
    let mut opens = [false; spec::OPTIONS.len()];
    for (slot, option) in opens.iter_mut().zip(spec::OPTIONS) {
        *slot = option.opens;
    }
    opens
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
    /// ★★★★★ R1697 — the panel's own size, which it did not have.
    ///
    /// The width and height were module constants, so every detached panel was
    /// the same size for ever and the corner could not be pulled. The reference
    /// gives each float its own `w`/`h` and a resize that clamps at
    /// [`FLOAT_MIN_W`] x [`FLOAT_MIN_H`] — measured by extracting its own
    /// source rather than inferred from the mockup.
    w: u32,
    h: u32,
    /// Which panel is in front, when two overlap.
    ///
    /// It was the vector's order, read backwards at the hit test — which works
    /// exactly until something has to change it, and bringing a panel forward
    /// is the first thing a press on one must do. Explicit, from a monotonic
    /// counter, because the reference's `raiseFloat` is what its drag calls
    /// first and the two are one gesture.
    z: u32,
}

/// A detached panel being moved or resized, in flight.
///
/// A separate type from [`Drag`] rather than an arm added to it, because the
/// two live on **different planes**: a card is dragged between the board's
/// cells and previews where it would land, and a panel is dragged in pixels and
/// moves as the pointer moves — which is what the reference does, and what a
/// window does. Folding a pixel gesture into a type whose fields are a cell
/// coordinate and a snap target would give one of them a meaning it cannot
/// carry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct FloatGrab {
    /// Which panel.
    id: String,
    /// Whether the pointer is moving the panel or sizing it.
    edge: bool,
    /// Where the pointer was when the grab opened.
    from: (u32, u32),
    /// The panel's position or size at that moment — the origin every
    /// subsequent delta is added to, so a drag is exact rather than accumulated
    /// (an accumulating drag drifts by one pixel per event that is swallowed).
    origin: (u32, u32),
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
    /// ★★★★★ R1695 — **where the rail has taken you**, and the roster it is
    /// taken from.
    ///
    /// This was a bare `Signal<String>` the rail highlighted itself from, and
    /// nothing else read it. Four of the seven seats therefore "navigated" to a
    /// dashboard: measured through the router, the press moved this string and
    /// left the painted scene at 193 tagged regions before and 193 after.
    ///
    /// A [`Journey`] instead, because the string could hold a destination no
    /// press could reach and had no way to refuse one — and the region below
    /// now reads it, so *arriving* is a fact about the window rather than about
    /// a variable.
    journey: Signal<Journey>,
    /// The roster the journey is navigated against. Held rather than rebuilt at
    /// each read so the paint, the hit test and the wire cannot be looking at
    /// three rosters — the failure this whole axis is a repair for.
    roster: Destinations,
    /// The Settings destination's four switches, in specification order.
    ///
    /// One array rather than four signals because the page renders them from a
    /// table and the wire publishes them from the same table; four fields would
    /// be four chances for the two to disagree about the order.
    options: Signal<[bool; spec::OPTIONS.len()]>,
    selected: Signal<Option<String>>,
    cursor: Signal<(u32, u32)>,
    pressed: RefCell<Option<Hit>>,
    drag: Signal<Option<Drag>>,
    /// ★★★★★ R1698 — **one keyboard cursor per composite**, keyed by the tag
    /// that owns the Tab stop.
    ///
    /// Held rather than rebuilt because a cursor is a position somebody put
    /// there: rebuilding it every frame would reset it whenever anything else
    /// on the screen changed. Re-seated (rather than replaced) each time it is
    /// read, so a roster that grows keeps the cursor on the member it was on.
    cursors: RefCell<BTreeMap<&'static str, Roving>>,
    /// R1697 — a detached panel being moved or sized, in flight.
    float_grab: Signal<Option<FloatGrab>>,
    /// R1697 — the next stacking number a raise hands out. Monotonic, as the
    /// reference's is: comparing two panels' `z` is only meaningful while the
    /// numbers are never reused.
    float_z: RefCell<u32>,
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
        let roster = spec::destinations();
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
            journey: Signal::new(
                Journey::begin(&roster, spec::RAIL_ACTIVE)
                    .expect("the screen opens at a destination it can reach"),
            ),
            roster,
            options: Signal::new(opening_options()),
            selected: Signal::new(None),
            cursor: Signal::new((0, 0)),
            pressed: RefCell::new(None),
            drag: Signal::new(None),
            cursors: RefCell::new(
                spec::FOCUS_RING
                    .iter()
                    .filter_map(|stop| stop.cursor.map(|spec| (stop.tag, Roving::new(spec))))
                    .collect(),
            ),
            float_grab: Signal::new(None),
            float_z: RefCell::new(0),
            toast: Signal::new(format!("{} loaded", spec::PRESET)),
            next_id: RefCell::new(u(spec::BOARD.len())),
            canvas_scroll: Rc::new(ScrollState::with_tag(CANVAS_SCROLL)),
        }
    }

    fn card(&self, id: &str) -> Option<Card> {
        self.cards.get().into_iter().find(|c| c.id().as_str() == id)
    }

    /// Where the rail has taken this window.
    fn at(&self) -> String {
        self.journey.get().at().to_owned()
    }

    /// Go to a destination the way both a press and the wire do.
    ///
    /// One function rather than one per channel: R1673 measured this screen's
    /// two paths giving a reserved seat two different answers, and a shared
    /// verb is the only arrangement in which they cannot.
    fn go(&self, key: &str) -> Result<(), Detour> {
        let mut journey = self.journey.get();
        let arrival = journey.navigate(&self.roster, key)?;
        let title = journey.here(&self.roster).title.clone();
        self.journey.set(journey);
        match arrival {
            pinion_core::widgets::destination::Arrival::AlreadyHere => {
                self.say(format!("already in {title}"));
            }
            pinion_core::widgets::destination::Arrival::Moved { .. } => {
                self.say(format!("{title} section"));
            }
        }
        Ok(())
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

    /// R1697 — the detached panels in stacking order, frontmost first.
    ///
    /// The hit test walks this and the paint walks its reverse, so the panel a
    /// press lands on is the panel drawn on top by construction rather than by
    /// two functions agreeing. Ties keep the roster's order, which only
    /// happens before anything has been raised.
    fn floats_front_to_back(&self) -> Vec<Float> {
        let mut floats = self.floats.get();
        floats.sort_by(|a, b| b.z.cmp(&a.z));
        floats
    }

    /// R1697 — hand out the next stacking number.
    ///
    /// The reference's `raiseFloat`, and its drag calls it first: bringing a
    /// panel forward is part of grabbing it, not a separate affordance.
    fn raise_float(&self, id: &str) {
        let next = {
            let mut counter = self.float_z.borrow_mut();
            *counter += 1;
            *counter
        };
        let floats = self
            .floats
            .get()
            .into_iter()
            .map(|f| {
                if f.id == id {
                    Float { z: next, ..f }
                } else {
                    f
                }
            })
            .collect();
        self.floats.set(floats);
    }

    /// R1697 — replace one panel, by id. Nothing if it is not detached.
    fn set_float(&self, id: &str, next: &Float) {
        let floats = self
            .floats
            .get()
            .into_iter()
            .map(|f| if f.id == id { next.clone() } else { f })
            .collect();
        self.floats.set(floats);
    }

    /// R1697 — one panel, by id.
    fn float(&self, id: &str) -> Option<Float> {
        self.floats.get().into_iter().find(|f| f.id == id)
    }

    /// ★★★★★ R1698 — **what a composite's arrows reach, in cursor order.**
    ///
    /// Derived from the same tables the paint and the accessibility tree read,
    /// so there is one roster rather than three. And deliberately NOT the
    /// accessibility children: the palette's children are three section groups
    /// and two status readouts while the thing a cursor walks is the thirteen
    /// catalogue entries inside them — the distinction the framework's `Roving`
    /// exists to keep, and the one the reference toolkit loses (its tab bar of
    /// three tabs reports five accessible children).
    ///
    /// A locked member stays in the roster. That is this screen's whole
    /// subject: a seat booked for a later release is SHOWN rather than hidden,
    /// so a reader must be able to put the cursor on it and be told what it is
    /// waiting for. The floor skips its disabled entries, which makes them
    /// undiscoverable from the keyboard.
    fn cursor_members(stop: &str) -> Vec<Member> {
        match stop {
            "shell.appbar" => vec![
                // ★★★★★ The two view tabs are a composite of their own, so the
                // bar reaches them as ONE member — WAI-ARIA's nesting, and the
                // reason a member is a tag rather than a control. R1698 stopped
                // there and R1699 measured what that cost: the tab list was
                // reachable and no key went into it, so from a keyboard the two
                // views could not be switched at all.
                Member::new(APP_BAR_TABS).containing(Self::view_tabs()),
                Member::new(BarChip::Source.tag()),
                Member::new(BarChip::Capture.tag()),
                Member::new(BarChip::Search.tag()),
            ],
            "shell.rail" => spec::RAIL
                .iter()
                .map(|seat| {
                    Member::maybe(
                        format!("shell.rail.{}", seat.key),
                        matches!(seat.seat, spec::Seat::Page),
                    )
                })
                .collect(),
            "shell.subbar" => SubChip::ALL
                .iter()
                .map(|chip| Member::new(chip.tag()))
                .collect(),
            "shell.palette" => spec::CATALOGUE
                .iter()
                .map(|def| {
                    Member::maybe(
                        format!("shell.palette.{}", def.kind),
                        def.tier == spec::Tier::Placeable,
                    )
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// ★★★★★ R1699 — the composite the application bar's first member **is**.
    ///
    /// A ring of two peers, so it wraps; and `Explicit`, because arriving at a
    /// view tab must not switch the view a reader is trying to leave — the same
    /// argument the rail's cursor makes, and the one the floor's tab list has
    /// no way to express (measured at 6.11.1: its tab bar changes the current
    /// tab on every arrow and exposes no property that would let an author ask
    /// for anything else).
    fn view_tabs() -> Roving {
        let mut tabs = Roving::new(
            RovingSpec::new(Axis::Horizontal)
                .with_ends(Ends::Wrap)
                .with_activation(Activation::Explicit),
        );
        tabs.seat(vec![
            Member::new(BarChip::Tab0.tag()),
            Member::new(BarChip::Tab1.tag()),
        ]);
        tabs
    }

    /// R1698 — the composite's cursor, re-seated from the live roster first.
    ///
    /// Re-seating rather than rebuilding is what keeps the cursor on the member
    /// somebody put it on when the roster changes underneath — the property
    /// `Roving::seat` exists for.
    fn with_cursor<R>(&self, stop: &str, f: impl FnOnce(&mut Roving) -> R) -> Option<R> {
        let members = Self::cursor_members(stop);
        let mut cursors = self.cursors.borrow_mut();
        let roving = cursors.get_mut(stop)?;
        roving.seat(members);
        Some(f(roving))
    }

    /// R1698 — a read-only snapshot of one composite's cursor, seated.
    fn cursor_of(&self, stop: &str) -> Option<Roving> {
        self.with_cursor(stop, |roving| roving.clone())
    }

    /// R1699 — a read-only snapshot of the composite one MEMBER is, seated.
    ///
    /// By member tag rather than "whatever the cursor rests on", because a
    /// nested composite publishes its roster whether or not anybody is standing
    /// on it: the point of publishing is to be askable before pressing a key.
    fn inner_cursor_of(&self, stop: &str, member: &str) -> Option<Roving> {
        self.with_cursor(stop, |roving| {
            roving
                .members()
                .iter()
                .find(|m| m.tag == member)
                .and_then(Member::inner)
                .cloned()
        })
        .flatten()
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

// ★★★★★ R1700 — the scope-free `Weak` handle that used to live here is GONE,
// and the compiler is what found it. R1671 introduced it for one reason: the
// two halves of this screen run in different places (a view is inside an
// `Owner` scope, the `External` invoke path is not) and `window_size` had to
// answer in both. `pinion_core::external::layout_size` answers in both now, so
// the handle had no remaining reader and `-D dead-code` said so on the first
// build after the switch.
//
// That is the shape of the repayment: the workaround did not have to be
// argued away, it stopped compiling.

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

/// The outline a card strokes inside its rectangle at rest.
const CARD_OUTLINE: u32 = 1;

/// The accent ring it strokes instead when it is the keyboard's subject — the
/// wider of the two, because a ring has to read as a ring.
const CARD_RING: u32 = 2;

/// The band a card reserves for its own frame: the **widest** the frame ever
/// gets, whichever of the two it is drawing right now.
///
/// ★★ R1671 — every one of a card's three bands is inset by it, and that is a
/// rule rather than three decisions. Reported by a person looking at the
/// window, twice: the stream's `time / type / name / len` strip sat on the
/// card's outline, and after the body was inset the outline was still eaten —
/// by the size-stepper strip, which is the band nobody had thought about.
///
/// ★★ R1672 — and it was [`CARD_OUTLINE`], so the moment a card became the
/// selected one and swapped its 1px outline for the 2px ring, its header and
/// **every row of its body** stood on the ring's second pixel. Nine marks, in a
/// state this screen's sweep already ran, and nothing could see them: until this
/// round `scene/containment` compared a mark against the owner's BOX, and a
/// border is ink the box owns *inside* that box, so painting over it was by
/// that definition contained. The channel has a content box now
/// ([`pinion_core::containment::content_rect`]) and this is the placement half.
///
/// Derived from the two widths rather than picked, and **constant** rather than
/// a function of `selected`: a content rectangle that changed with the ring
/// would move every row in the card by a pixel when a person selects it.
const CARD_FRAME: u32 = if CARD_RING > CARD_OUTLINE {
    CARD_RING
} else {
    CARD_OUTLINE
};

/// The card's header band — the grip, the title and the header controls.
const fn header_rect(card: Rect) -> Rect {
    Rect::new(
        card.x + CARD_FRAME,
        card.y + CARD_FRAME,
        card.w.saturating_sub(CARD_FRAME * 2),
        CARD_HDR.saturating_sub(CARD_FRAME),
    )
}

/// The card's content band, between the header and the edit strip.
const fn body_rect(card: Rect, editing: bool) -> Rect {
    let foot = if editing { EDIT_BAR_H } else { 0 };
    Rect::new(
        card.x + CARD_FRAME,
        card.y + CARD_HDR,
        card.w.saturating_sub(CARD_FRAME * 2),
        card.h.saturating_sub(CARD_HDR + foot + CARD_FRAME),
    )
}

/// The size-stepper strip at the foot of a card in layout-edit mode.
const fn edit_bar_rect(card: Rect) -> Rect {
    Rect::new(
        card.x + CARD_FRAME,
        (card.y + card.h).saturating_sub(EDIT_BAR_H + CARD_FRAME),
        card.w.saturating_sub(CARD_FRAME * 2),
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

    fn rect(self) -> Rect {
        match self {
            Self::Tab0 => Rect::new(168, 10, 108, 32),
            Self::Tab1 => Rect::new(280, 10, 118, 32),
            Self::Source => Rect::new(416, 10, 268, 32),
            Self::Capture => Rect::new(696, 10, 132, 32),
            Self::Search => Rect::new(win_w() - 300, 10, 288, 32),
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
    fn rect(self) -> Rect {
        let bar_w = win_w() - RAIL_W - PALETTE_W;
        match self {
            Self::Preset => Rect::new(16, 7, 178, 32),
            Self::EditLayout => Rect::new(bar_w - 330, 7, 140, 32),
            Self::AddWidget => Rect::new(bar_w - 180, 7, 164, 32),
        }
    }
}

/// One entry of the open preset menu, in **window** space.
///
/// ★★ R1672 — window rather than the sub bar's, because the menu left the sub
/// bar. A drop-down is anchored to the control that opens it and is not
/// *bounded* by it: this one hung 81 pixels below the bar it was a child of,
/// which the ink gate reports as an escape and which is exactly what it is —
/// the reference toolkit at 6.11 makes a menu a top-level popup for the same
/// reason. It is now a sibling of the bars, painted after them, at the window
/// coordinates its anchor derives.
fn preset_item_rect(n: u32) -> Rect {
    let anchor = SubChip::Preset.rect();
    Rect::new(
        RAIL_W + anchor.x + 8,
        APP_BAR_H + anchor.y + 44 + n * 34,
        210,
        30,
    )
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
fn palette_rect() -> Rect {
    Rect::new(
        win_w() - PALETTE_W,
        APP_BAR_H,
        PALETTE_W,
        win_h() - APP_BAR_H,
    )
}

/// The palette's rows — section headers interleaved with entries, in the
/// panel's own space.
///
/// Returned rather than recomputed at each site: the painter walks it to draw
/// and the hit test walks it to resolve, which is the discipline the card
/// rectangles follow.
fn palette_rows() -> Vec<PaletteRow> {
    let mut out = Vec::new();
    let mut y = 76_u32;
    for (key, title, _tier) in spec::SECTIONS {
        out.push(PaletteRow {
            def: None,
            section: key,
            title,
            rect: Rect::new(16, y, PALETTE_W - 32, 20),
        });
        y += 26;
        for def in spec::CATALOGUE.iter().filter(|w| w.section == *key) {
            out.push(PaletteRow {
                def: Some(def),
                section: key,
                title: def.label,
                rect: Rect::new(10, y, PALETTE_W - 30, PALETTE_ROW_H),
            });
            y += PALETTE_ROW_H + 4;
        }
        y += 8;
    }
    out
}

/// One line of the palette panel: a section heading, or a catalogue entry.
struct PaletteRow {
    /// The catalogue entry, or `None` for the section heading above them.
    def: Option<&'static spec::WidgetSpec>,
    /// The section this line belongs to — the heading's own key, and the key of
    /// the section an entry sits under. Carried rather than recomputed because
    /// the heading is the group a reader descends through, and it needs a tag.
    section: &'static str,
    /// The words painted on the line.
    title: &'static str,
    /// Where, in the palette panel's own space.
    rect: Rect,
}

// --- What is under a point ---------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hit {
    Chip(BarChip),
    Sub(SubChip),
    PresetItem(usize),
    Rail(&'static str),
    /// R1695 — a Settings switch, by its specification key.
    Option(&'static str),
    /// R1695 — a Settings key row's button, which is booked for a later release.
    KeyRow(&'static str),
    /// R1695 — a theme segment, by its index in [`spec::THEMES`].
    Theme(usize),
    Palette(&'static str),
    Grip(String),
    Affordance(String, CardAffordance),
    Stepper(String, &'static str),
    Remedy(String),
    Card(String),
    FloatRedock(String),
    FloatClose(String),
    /// R1697 — the corner that sizes a detached panel. Its own arm rather than
    /// a modifier on [`Self::Float`], because the reference stops the event
    /// propagating there: a grab on the corner must not also start a move.
    FloatResize(String),
    Float(String),
    Nothing,
}

impl Hit {
    /// Front to back: the preset menu is over the sub bar, floats are over the
    /// canvas, and a card's own controls are over its body.
    fn at(state: &ShellState, px: u32, py: u32) -> Self {
        // ★ R1672 — the menu is a top-level popup now, so its rows are asked
        // for in WINDOW coordinates and there is no origin to subtract. The
        // paint reads the same function.
        if state.preset_open.get() {
            let rows = state.presets.borrow().len();
            for n in 0..=rows {
                if contains(preset_item_rect(u(n)), px, py) {
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
        // ★★ R1695 — the rail is chrome and is asked first, whatever the page.
        // Everything after it belongs to the destination showing, which is why
        // this branch moved above the palette's: the palette is the dashboard's
        // and is not painted anywhere else, so a press in its old column at
        // another destination must fall through to that page rather than reach a
        // row nobody can see. The paint and this test read one `page_rect`.
        if px < RAIL_W {
            for (n, seat) in spec::RAIL.iter().enumerate() {
                if contains(rail_rect(u(n)), px, py - APP_BAR_H) {
                    return Self::Rail(seat.key);
                }
            }
            return Self::Nothing;
        }
        let at = state.at();
        if at != "dashboard" {
            let region = page_rect(&at);
            return Self::in_settings(state, px - region.x, py - region.y);
        }
        if px >= palette_rect().x {
            let panel = palette_rect();
            let (lx, ly) = (px - panel.x, py - panel.y);
            for row in palette_rows() {
                if let Some(def) = row.def
                    && contains(row.rect, lx, ly)
                {
                    return Self::Palette(def.kind);
                }
            }
            return Self::Nothing;
        }
        if py < APP_BAR_H + SUB_BAR_H {
            let (lx, ly) = (px - RAIL_W, py - APP_BAR_H);
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

    /// ★★★★★ R1699 — what a **key** press at `tag` addresses: the inverse of
    /// [`hit_word`].
    ///
    /// A keyboard activation is semantic, not geometric — the reader named a
    /// thing, not a pixel — so synthesising a press at the middle of the tag's
    /// rectangle would be wrong twice over: a member scrolled out of view has a
    /// rectangle and is covered, and a member with no rectangle at all (the
    /// tab list, which is anchored by the tabs it composes) has none to aim at.
    ///
    /// The risk of an inverse is that it drifts from the thing it inverts, so
    /// the round wrote the gate before the function:
    /// `r1699_every_cursor_member_resolves_to_the_hit_its_tag_names` requires,
    /// for every member of every composite, that `hit_word(of_tag(t)) == t`
    /// AND that `of_tag(t)` equals what `Hit::at` answers at the centre of that
    /// tag's painted rectangle. Two independent derivations of the same fact,
    /// with the paint as the arbiter.
    fn of_tag(state: &ShellState, tag: &str) -> Self {
        if let Some(chip) = BarChip::ALL.into_iter().find(|c| c.tag() == tag) {
            return Self::Chip(chip);
        }
        if let Some(chip) = SubChip::ALL.into_iter().find(|c| c.tag() == tag) {
            return Self::Sub(chip);
        }
        if let Some(key) = tag.strip_prefix("shell.rail.")
            && let Some(seat) = spec::RAIL.iter().find(|seat| seat.key == key)
        {
            return Self::Rail(seat.key);
        }
        if let Some(kind) = tag.strip_prefix("shell.palette.")
            && let Some(def) = spec::CATALOGUE.iter().find(|def| def.kind == kind)
        {
            return Self::Palette(def.kind);
        }
        if let Some(key) = tag.strip_prefix("shell.settings.option.")
            && let Some(option) = spec::OPTIONS.iter().find(|o| o.key == key)
        {
            return Self::Option(option.key);
        }
        if let Some(key) = tag.strip_prefix("shell.settings.key.")
            && let Some(row) = spec::KEY_ROWS.iter().find(|row| row.key == key)
        {
            return Self::KeyRow(row.key);
        }
        if let Some(n) = tag
            .strip_prefix("shell.settings.theme.")
            .and_then(|n| n.parse::<usize>().ok())
            && n < spec::THEMES.len()
        {
            return Self::Theme(n);
        }
        if let Some(n) = tag
            .strip_prefix("shell.preset.item.")
            .and_then(|n| n.parse::<usize>().ok())
            && n <= state.presets.borrow().len()
        {
            return Self::PresetItem(n);
        }
        Self::Nothing
    }

    /// ★ R1695 — what is under a point on the Settings page, in the region's own
    /// space.
    ///
    /// The rectangles come from the same `settings_*_rect` helpers the painter
    /// uses, which is the standing rule on this screen: what is drawn and what
    /// responds are derived from ONE fact.
    fn in_settings(state: &ShellState, cx: u32, cy: u32) -> Self {
        let region = page_rect(&state.at());
        for (n, option) in spec::OPTIONS.iter().enumerate() {
            let seat = Self::option_seat(region, n);
            if contains(seat, cx, cy) {
                return Self::Option(option.key);
            }
        }
        for (n, row) in spec::KEY_ROWS.iter().enumerate() {
            let card = settings_group_rect(region, "keys");
            let local_row = Rect::new(0, u(n) * SET_ROW_H, card.w, SET_ROW_H);
            let seat = settings_ctrl_rect(local_row, SET_CTRL_W);
            if contains(
                Rect::new(card.x + seat.x, card.y + seat.y, seat.w, seat.h),
                cx,
                cy,
            ) {
                return Self::KeyRow(row.key);
            }
        }
        for n in 0..spec::THEMES.len() {
            if contains(Self::theme_seat(region, u(n)), cx, cy) {
                return Self::Theme(n);
            }
        }
        Self::Nothing
    }

    /// The region-space rectangle of the `n`th switch on the Settings page.
    fn option_seat(region: Rect, n: usize) -> Rect {
        let option = &spec::OPTIONS[n];
        let card = settings_group_rect(region, option.group);
        let within = u(spec::OPTIONS[..n]
            .iter()
            .filter(|o| o.group == option.group)
            .count());
        let row = Rect::new(0, within * SET_ROW_H, card.w, SET_ROW_H);
        let seat = settings_ctrl_rect(row, 64);
        Rect::new(card.x + seat.x, card.y + seat.y, seat.w, seat.h)
    }

    /// The region-space rectangle of a theme segment.
    fn theme_seat(region: Rect, n: u32) -> Rect {
        let card = settings_group_rect(region, "appearance");
        let seat = settings_ctrl_rect(Rect::new(0, 0, card.w, SET_ROW_H), SEG_W);
        let w = seg_chip_w();
        Rect::new(
            card.x + seat.x + SEG_PAD + n * w,
            card.y + seat.y + 1 + SEG_PAD,
            w,
            SEG_CHIP_H,
        )
    }

    /// What is under a point in the canvas's own space.
    ///
    /// Split out of [`Self::at`] because the canvas is the one region with a
    /// stacking order of its own — floats over cards, a card's controls over
    /// its body — and reading that order should not mean scrolling past the
    /// four chrome regions first.
    fn in_canvas(state: &ShellState, cx: u32, cy: u32) -> Self {
        // ★ R1697 — floats are over the canvas, FRONTMOST first. It was the
        // vector read backwards, which is the same answer only while nothing
        // reorders them; a press now raises the panel it lands on, so the
        // stacking order is state and the hit test reads that state.
        for float in state.floats_front_to_back() {
            let rect = float_rect(&float);
            if !contains(rect, cx, cy) {
                continue;
            }
            // The corner is tested before the body, because it is inside it.
            // ★ R1681.3's invariant, and this is a case of it: a new affordance
            // drawn over an existing one has to be reached before it, and the
            // paint draws the grip last for the same reason.
            if contains(float_grip_rect(&float), cx, cy) {
                return Self::FloatResize(float.id.clone());
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
///
/// ★ R1697 — the one function the paint AND the hit test read, kept that way
/// deliberately ([[debt-paint-and-gesture-read-two-facts]]): a resizable panel
/// whose corner is computed twice is a corner that can be drawn in one place
/// and grabbed in another.
const fn float_rect(float: &Float) -> Rect {
    Rect::new(float.x, float.y, float.w, float.h)
}

/// The corner a resize is grabbed by, in the same space as [`float_rect`].
const fn float_grip_rect(float: &Float) -> Rect {
    let rect = float_rect(float);
    Rect::new(
        rect.x + rect.w.saturating_sub(FLOAT_GRIP),
        rect.y + rect.h.saturating_sub(FLOAT_GRIP),
        FLOAT_GRIP,
        FLOAT_GRIP,
    )
}

/// One hit, named by the **scene tag** of the thing that was hit.
///
/// R1614's lesson — a name that has to survive is an address, not a
/// description — and the demo enforces it by sweeping the window and requiring
/// every name this returns to be a tag the paint actually emitted.
/// ★ R1700 — the same word, in the shape the framework's pointer census reads:
/// `Nothing` where a press addresses nothing, so that "there is nothing here"
/// and "here is a thing called nothing" cannot be confused.
fn word_or_nothing(hit: &Hit) -> PointerTarget {
    match hit {
        Hit::Nothing => PointerTarget::Nothing,
        other => PointerTarget::Word(hit_word(other)),
    }
}

fn hit_word(hit: &Hit) -> String {
    match hit {
        Hit::Chip(chip) => chip.tag().to_string(),
        Hit::Sub(chip) => chip.tag().to_string(),
        Hit::PresetItem(n) => format!("shell.preset.item.{n}"),
        Hit::Rail(name) => format!("shell.rail.{name}"),
        Hit::Option(key) => format!("shell.settings.option.{key}"),
        Hit::KeyRow(key) => format!("shell.settings.key.{key}"),
        Hit::Theme(n) => format!("shell.settings.theme.{n}"),
        Hit::Palette(kind) => format!("shell.palette.{kind}"),
        Hit::Grip(id) => format!("card.{id}.grip"),
        Hit::Affordance(id, affordance) => format!("card.{id}.{}", affordance.wire()),
        Hit::Stepper(id, verb) => format!("card.{id}.{verb}"),
        Hit::Remedy(id) => format!("card.{id}.remedy"),
        Hit::Card(id) => format!("card.{id}"),
        Hit::FloatRedock(id) => format!("float.{id}.redock"),
        Hit::FloatClose(id) => format!("float.{id}.close"),
        Hit::FloatResize(id) => format!("float.{id}.resize"),
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

    fn new() -> Self {
        Self {
            state: None,
            surface: window_size(),
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
            // ★★★★★ R1697 — the header control **toggles**, and it did not.
            //
            // Found by this round's own operations gate on its first run: the
            // card was maximised with a press and the same press then called
            // `maximize` again, which refuses with "a card is already
            // maximised; restore first". So a person who maximised a card with
            // the mouse had no way back with the mouse — only `Escape`, or the
            // wire. Every window control that grows a thing shrinks it again;
            // nothing had ever asked this one to.
            //
            // The two WIRE verbs stay separate and precise (`maximize` and
            // `restore` each refuse when they do not apply), because an agent
            // asking for a specific outcome should not get a toggle. This is
            // the affordance, and an affordance is a press.
            CardAffordance::Maximize => {
                return if state.maximized.get().is_some_and(|m| m.id().as_str() == id) {
                    Self::restore(state)
                } else {
                    Self::maximize(state, id)
                };
            }
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
        let z = {
            let mut counter = state.float_z.borrow_mut();
            *counter += 1;
            *counter
        };
        let mut floats = state.floats.get();
        floats.push(Float {
            id: id.to_string(),
            x: 120 + n * FLOAT_STEP,
            y: 40 + n * FLOAT_STEP,
            // R1697 — the reference's opening size, per panel from here on
            // rather than for all of them at once.
            w: FLOAT_W,
            h: FLOAT_H,
            // A panel arrives in front, which is also what its `detachWidget`
            // does — it takes `floatZ + 1` in the same breath as its position.
            z,
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
        // ★★ R1695 — where the rail can take you, what each seat's standing is,
        // and which one is showing. `nav` above says only the last of those.
        SchemaField::new("destinations", "json"),
        // The Settings destination's switches.
        SchemaField::new("options", "json"),
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
        SchemaField::new("floats", "json"),
        SchemaField::new("float_grab", "string"),
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

/// R1697 — every detached panel's geometry, front to back.
///
/// Its own function so the reply is one place and so the read arm stays inside
/// the length the lints allow; the order IS the stacking order, so a client
/// never has to sort for it.
fn floats_json(state: &ShellState) -> serde_json::Value {
    serde_json::Value::Array(
        state
            .floats_front_to_back()
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id, "x": f.x, "y": f.y, "w": f.w, "h": f.h, "z": f.z,
                })
            })
            .collect(),
    )
}

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
            "nav" => text(state.at()),
            // ★★ R1695 — the roster and the position, in one published value
            // built by the framework so two screens of one product cannot
            // publish the same fact in two shapes.
            "destinations" => Ok(IntrospectValue::Json(
                state.roster.wire(&state.journey.get()),
            )),
            "options" => Ok(IntrospectValue::Json(options_json(state))),
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
            // ★★★★★ R1697 — **where each detached panel is, how big, and which
            // is in front**, which nothing published.
            //
            // `floating` above is the roster and answers "which cards have
            // left the board"; this is their geometry, and without it a moved
            // panel is invisible to every reader that is not a screenshot.
            // That is the whole reason the defect survived: the operation had
            // no witness a test could name, so no test could name it.
            //
            // Front to back, so the order IS the stacking order rather than
            // something a reader has to sort for.
            "floats" => Ok(IntrospectValue::Json(floats_json(state))),
            // What the pointer is doing to a panel right now, or empty. The
            // peer of `drag` below, and separate for the reason the types are
            // separate: one gesture lands in a cell and the other in a pixel.
            "float_grab" => text(state.float_grab.get().map_or_else(String::new, |g| {
                format!("{},{}", g.id, if g.edge { "resize" } else { "move" })
            })),
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
            // ★ R1695 — the wire drives the SAME verb the pointer does, so the
            // two channels cannot answer a destination differently. What the
            // wire adds is the closed set in its refusal, which a person gets
            // from the rail itself.
            "nav" => {
                let name = word(&value)?;
                state.go(&name).map_err(|detour| match detour {
                    Detour::NoSuchDestination { .. } => InterveneError::out_of_range(format!(
                        "{name:?} is not a rail section; they are {}",
                        state.roster.keys().collect::<Vec<_>>().join(", ")
                    )),
                    Detour::Closed { .. } => InterveneError::out_of_range(format!(
                        "the {name:?} section is {}",
                        state
                            .roster
                            .get(&name)
                            .and_then(|d| d.standing.why())
                            .map_or_else(
                                String::new,
                                pinion_core::availability::Unavailable::sentence
                            )
                    )),
                })
            }
            "preset" => ShellOracle::apply_preset(&state, &word(&value)?),
            "sources" | "cards" | "card_count" | "placed_count" | "layout" | "maximized"
            | "restore_to" | "floating" | "floats" | "float_grab" | "presets" | "transport"
            | "playhead" | "affordances" | "states" | "remedies" | "steppers" | "toast"
            | "cursor" | "selected" | "hit" | "keymap" | "rail" | "tabs" | "catalogue"
            | "config_open" | "drag" => Err(InterveneError::ReadOnly),
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
                // ★ R1671 — from the STATE, not through the Owner-scoped read:
                // this path has no scope, so `window_size()` here answered with
                // the design fallback and refused presses the paint had put on
                // screen.
                let (w, h) = window_size();
                if x >= w || y >= h {
                    return Err(InvokeError::rejected(format!(
                        "({x},{y}) is outside the {w}x{h} shell"
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
        if let Some(grab) = state.float_grab.get() {
            Self::carry_float_grab(state, &grab, px, py);
            return;
        }
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

    /// R1698 — what a composite's cursor landing somewhere does besides move.
    ///
    /// Every stop here declares
    /// [`Activation::Explicit`],
    /// so arriving never chooses — and the `choose` bit is read rather than
    /// ignored, because a declaration nothing reads is the "declared and thrown
    /// away" arm R1684 named. If a stop is ever declared `Follows`, this is
    /// where that becomes an action rather than a comment.
    ///
    /// The toast is what makes the cursor perceivable at all to somebody
    /// driving the wire, and the ring is what makes it perceivable on screen:
    /// the framework frames the active descendant, which this screen publishes
    /// through `access_focus_target`.
    fn landed(state: &Rc<ShellState>, stop: &str, landing: Landing) {
        let Some(tag) = state
            .cursor_of(stop)
            .and_then(|r| r.active_descendant().map(str::to_owned))
        else {
            return;
        };
        let what = match landing {
            // ★ Every stop on THIS screen declares `Explicit`, and a unit test
            // asserts that, so a `choose: true` reaching here would mean the
            // specification changed without this arm being written. The other
            // arm's real consumer is the capture viewer's message list, where
            // the cursor IS the selection.
            Landing::Moved { choose: true, .. } => "cursor moved and chose",
            Landing::Moved { .. } => "cursor moved",
            Landing::Held(_) => "cursor held at the end",
            Landing::Nowhere => return,
            // ★★★★★ R1699 — the half `Explicit` had always promised and never
            // delivered. Measured before this arm existed, by driving the
            // running screen: `Enter` and `Space` at all five stops, twelve
            // presses, nothing painted changed.
            //
            // Both arms reach `act_on_hit`, which is the ONE place that knows
            // what a tag does and why it might refuse — a second refusal
            // sentence written here would be the two-spellings defect this
            // screen has already paid for twice (R1695's rail, R1668's seats).
            // They stay distinct on the wire, where `enabled` tells a client
            // that choosing this member refuses *before* it presses anything.
            Landing::Chosen(_) | Landing::Refused(_) => {
                Self::act_on_hit(state, Hit::of_tag(state, &tag));
                return;
            }
            Landing::Entered(_) => "entered",
            Landing::Exited(_) => "left",
        };
        state.say(format!("{tag} \u{00B7} {what}"));
    }

    /// R1697 — bring a detached panel forward and start moving or sizing it.
    ///
    /// The raise comes first because the reference's `startFloatDrag` calls
    /// `raiseFloat` before it reads the panel's origin, and the order is load
    /// bearing: grabbing a panel that is behind another must bring it out from
    /// under before the pointer starts moving it, or the panel being dragged is
    /// the one the person cannot see.
    fn open_float_grab(state: &Rc<ShellState>, id: &str, edge: bool, at: (u32, u32)) {
        state.raise_float(id);
        let Some(float) = state.float(id) else {
            return;
        };
        state.float_grab.set(Some(FloatGrab {
            id: id.to_string(),
            edge,
            from: at,
            origin: if edge {
                (float.w, float.h)
            } else {
                (float.x, float.y)
            },
        }));
    }

    /// R1697 — carry a float grab to the cursor.
    ///
    /// Live rather than previewed, which is the opposite of what a card drag
    /// does on this same screen and is correct for both: a card lands in a cell
    /// and the board would reflow under the finger, and a panel lands where it
    /// is put. The reference draws the same line.
    fn carry_float_grab(state: &Rc<ShellState>, grab: &FloatGrab, px: u32, py: u32) {
        let Some(float) = state.float(&grab.id) else {
            return;
        };
        // Signed, because a panel can be dragged left and up as readily as
        // right and down, and unsigned arithmetic would clamp a leftward drag
        // to zero movement rather than to the window edge.
        let dx = i64::from(px) - i64::from(grab.from.0);
        let dy = i64::from(py) - i64::from(grab.from.1);
        let shift = |origin: u32, delta: i64, floor: u32| -> u32 {
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "clamped into u32's range on the line above the cast"
            )]
            let moved =
                (i64::from(origin) + delta).clamp(i64::from(floor), i64::from(u32::MAX)) as u32;
            moved
        };
        let next = if grab.edge {
            Float {
                w: shift(grab.origin.0, dx, FLOAT_MIN_W),
                h: shift(grab.origin.1, dy, FLOAT_MIN_H),
                ..float
            }
        } else {
            Float {
                x: shift(grab.origin.0, dx, 0),
                y: shift(grab.origin.1, dy, 0),
                ..float
            }
        };
        state.set_float(&grab.id, &next);
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
        // ★★★★★ R1697 — **a press on a detached panel grabs it.**
        //
        // This is the arm a person reported: it read
        // `Hit::Float(_) | Hit::Nothing => {}` on the release path and nothing
        // opened a gesture on the press path, so a torn-off panel was nailed
        // where it landed. Every gate on this screen was green and each was
        // correct — the panel is painted, hit-testable, named and announced.
        // None of them asks whether grabbing it moves it.
        if let Hit::Float(id) | Hit::FloatResize(id) = &hit {
            Self::open_float_grab(state, id, matches!(hit, Hit::FloatResize(_)), (px, py));
        }
        *state.pressed.borrow_mut() = Some(hit);
    }

    /// A release performs the latched control if the cursor is still on it, and
    /// commits a drag wherever the preview ended up.
    fn release(state: &Rc<ShellState>) {
        let latched = state.pressed.borrow_mut().take();
        // ★ R1697 — a float grab has already done its work by the time the
        // button comes up (it moves live), so releasing only ends it. Returning
        // here is what stops the release ALSO acting on the latched hit: a drag
        // that finished over the panel's body must not read as a press on the
        // body.
        if let Some(grab) = state.float_grab.get() {
            state.float_grab.set(None);
            // Only speak if something actually changed. A press and release
            // without movement is a click on the panel, and announcing "moved"
            // for it would be the same class of lie the rail told before R1695:
            // a message describing an arrival that did not happen.
            let now = state
                .float(&grab.id)
                .map(|f| if grab.edge { (f.w, f.h) } else { (f.x, f.y) });
            if now.is_some_and(|now| now != grab.origin) {
                state.say(format!(
                    "{} {}",
                    label_of(&grab.id),
                    if grab.edge { "resized" } else { "moved" }
                ));
            }
            return;
        }
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
            // ★ R1695 — one verb, and its refusal is the roster's sentence.
            // Before this the press path authored its own message and told a
            // reserved seat the wrong thing on the sibling screen; a refusal
            // nobody derives is a refusal two channels will spell two ways.
            Hit::Rail(key) => {
                if let Err(detour) = state.go(key) {
                    state.say(detour.sentence(&state.roster));
                }
            }
            Hit::Option(key) => Self::toggle_option(state, key),
            // Painted inert, so a pointer never reaches it; this is the
            // keyboard and wire path saying the same thing the seat declares.
            Hit::KeyRow(key) => {
                let row = spec::KEY_ROWS.iter().find(|row| row.key == key);
                if let Some(row) = row {
                    state.say(format!(
                        "{} is {}",
                        row.title,
                        Unavailable::reserved(row.reserved_for).sentence()
                    ));
                }
            }
            Hit::Theme(n) => Self::choose_theme(state, n),
            Hit::Palette(kind) => {
                if let Err(why) = Self::add(state, kind) {
                    state.say(refusal_sentence(&why));
                }
            }
            Hit::Affordance(id, affordance) => {
                let call = IntrospectValue::Text(format!("{id},{}", affordance.wire()));
                if let Err(why) = Self::act(state, &call) {
                    // A refusal a person triggered has to be visible to that
                    // person, not only to the wire that would have read it.
                    state.say(refusal_sentence(&why));
                }
            }
            Hit::Stepper(id, verb) => {
                if let Err(why) = Self::step(state, &id, verb) {
                    state.say(refusal_sentence(&why));
                }
            }
            Hit::Remedy(id) => Self::apply_remedy(state, &id),
            Hit::FloatRedock(id) => {
                if let Err(why) = Self::redock(state, &id) {
                    state.say(refusal_sentence(&why));
                }
            }
            Hit::FloatClose(id) => {
                Self::remove(state, &id);
                state.say(format!("{} closed", label_of(&id)));
            }
            Hit::Card(id) | Hit::Grip(id) => state.say(format!("{} selected", label_of(&id))),
            // A press on a panel raised and grabbed it (`open_float_grab`);
            // there is nothing left for the release to do, and the raise is
            // why this is no longer the same arm as hitting nothing.
            Hit::Float(_) | Hit::FloatResize(_) | Hit::Nothing => {}
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
                // what SAYS where widgets come from rather than opening a second
                // chooser.
                //
                // ★ R1695 — it used to move the rail's highlight to `catalog`
                // as well, which is this round's own defect in miniature: the
                // rail said you were at a destination the window had not taken
                // you to. The pointer is what the button aims, and it aims at
                // the palette on this page.
                state.say("pick a widget from the palette \u{2192}");
            }
        }
    }

    /// ★ R1695 — flip one Settings switch.
    ///
    /// Keyed by the specification's own order rather than by a field per option,
    /// so a fifth switch is a row in the table and nothing here.
    fn toggle_option(state: &Rc<ShellState>, key: &str) {
        let Some(n) = spec::OPTIONS.iter().position(|o| o.key == key) else {
            return;
        };
        let mut on = state.options.get();
        on[n] = !on[n];
        state.options.set(on);
        state.say(format!(
            "{} {}",
            spec::OPTIONS[n].title,
            if on[n] { "on" } else { "off" }
        ));
    }

    /// ★ R1695 — choose a theme from the Settings page's segment.
    ///
    /// The same `ThemeProvider` the application bar's toggle and the `t` chord
    /// write, so three affordances over one fact rather than three facts.
    fn choose_theme(state: &Rc<ShellState>, n: usize) {
        let dark = n == 0;
        state.theme.set_mode(if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        });
        state.say(format!("theme {}", spec::THEMES[n].to_lowercase()));
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
        Self::key_at(state, focus_state::focused().as_deref(), chord)
    }

    /// ★★★★★ R1698 — **the keymap, told where the reader is standing.**
    ///
    /// Before this round the screen's keyboard was global: an arrow meant "move
    /// the board's selection" wherever focus was, so Tabbing to the rail and
    /// pressing Down moved a card on a board the reader had left. That is half
    /// of the WAI-ARIA composite pattern missing — R1696 gave each composite one
    /// Tab stop and nothing gave it a cursor inside.
    ///
    /// So the composite that owns the focus is asked FIRST, and only a key it
    /// does not navigate by falls through to the screen. A composite declaring
    /// [`Axis::Horizontal`]
    /// returns `None` for `ArrowUp`, which is what lets a vertical enclosing
    /// gesture still work while the reader is inside a horizontal bar.
    fn key_at(state: &Rc<ShellState>, focused: Option<&str>, chord: &str) -> bool {
        if let Some(stop) = focused
            && let Some(landing) = state
                .with_cursor(stop, |roving| roving.key(chord))
                .flatten()
        {
            Self::landed(state, stop, landing);
            return true;
        }
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
            // ★★★★★ R1698 — **the board's arrows belong to the board.**
            //
            // A key a composite does not navigate by falls through, which is
            // right; what is not right is where it used to land. Measured after
            // the cursors went in: standing on the application bar and pressing
            // Down moved a CARD on the board the reader had left, because this
            // handler had no idea where anybody was standing.
            //
            // `None` still reaches it, and deliberately: that is the wire's own
            // channel (`invoke("key", …)` with nothing focused) and an agent
            // asking the board to move its selection is asking for exactly
            // that. What is refused is an arrow arriving from inside another
            // composite.
            if focused.is_some_and(|tag| tag != "shell.canvas") {
                return false;
            }
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
        // ★ R1700 — only this surface's own field, which is what a pointer
        // fraction is a fraction of. R1671 also wrote the size into the state
        // because the geometry helpers had no other way to read it off a view
        // scope; `layout_size` reads the framework's own record instead, so
        // the copy on the state has gone with the handle that reached it.
        self.surface = (width.max(1), height.max(1));
    }

    /// ★★★★★ R1700 §5.35 — what a press here addresses, for the framework to
    /// hold against what this screen painted here.
    fn target_at(&self, x: u32, y: u32) -> PointerTarget {
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            word_or_nothing(&Hit::at(s, x, y))
        })
    }

    /// ★★★★★ R1700 §5.35 — the same question by name, over [`Hit::of_tag`].
    ///
    /// R1699 built that inverse for keyboard activation and gated it against
    /// the paint, so it is not derived from the geometry this is checked
    /// against — which is what makes the pair two derivations rather than one
    /// read twice.
    fn target_of_tag(&self, tag: &str) -> PointerTarget {
        self.state.as_ref().map_or(PointerTarget::Unanswered, |s| {
            word_or_nothing(&Hit::of_tag(s, tag))
        })
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
/// ★★ R1672 — and the run **elides**, which this screen alone did not say.
///
/// A run placed at an exact rectangle has a fixed width, so what happens when
/// the string is longer than that width is a policy somebody has to choose.
/// Screens A and B choose `Ellipsis` for every run they place; this screen left
/// the default, so a label too long for its box was painted *past* it — over
/// whatever was next — rather than shortened. Measured the round the ink gate
/// reached this screen: **32 marks**, including four of the app bar's own.
///
/// `Ellipsis` is the honest contract for a fixed box: it says the box wins and
/// the string gives way. A box that is genuinely too small for its string is
/// still a defect, and it is the shaped measurement at boot
/// (`scene/containment`) that reports that one — this policy is about what the
/// paint does when it happens, not about pretending it did not.
fn label(text: &str, rect: Rect, px: u32, fg: Color) -> Scene {
    clipped(text, rect, px, fg, TextOverflow::Ellipsis)
}

/// A kind's three-letter code in its coloured chip, the word seated by the
/// chip.
///
/// ★ R1672 — one helper because two call sites drew this and **both put the
/// word's box past the chip's right edge**, by the same three pixels, from two
/// separately picked insets (`(5, 9, 30, 14)` in a 32-wide chip and
/// `(9, 9, 34, 14)` in a 40-wide one). Neither is a judgement anybody made
/// twice; it is one rule written twice and got wrong twice, which is the
/// mechanical-duplication case.
fn code_chip(code: &str, chip: Rect, skin: BoxStyle, ink: Color, tag: Option<String>) -> Scene {
    /// The clearance a code chip keeps between its edge and its word.
    const CODE_PAD: u32 = 5;
    let line = pinion_core::containment::line_box(FONT_TINY);
    let word = Rect::new(
        CODE_PAD,
        chip.h.saturating_sub(line) / 2,
        chip.w.saturating_sub(CODE_PAD * 2),
        line,
    );
    let node = ContainerNode::new(vec![label(code, word, FONT_TINY, ink)]);
    let node = match tag {
        Some(tag) => node.with_tag(tag),
        None => node,
    };
    Scene::Container(node.with_style(skin).with_layout(absolute(chip)))
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

// --- The Settings destination ------------------------------------------------
//
// ★★ R1695 — the reference's own Settings section, reproduced: four switch rows
// in two groups, two rows whose affordance is booked for a later release, and a
// two-way appearance segment. The switch is
// [`pinion_widget_paint::switch`](switch::view_switch) rather than a hand-drawn
// track — the class R1673 measured on screen A, where a track was painted with
// no knob at all because the screen drew a switch instead of using one.

/// The Settings page's metrics, region-local.
const SET_PAD: u32 = 24;
const SET_MAX_W: u32 = 720;
const SET_HEAD_H: u32 = 14;
const SET_HEAD_GAP: u32 = 10;
const SET_GROUP_GAP: u32 = 22;
const SET_ROW_H: u32 = 64;
const SET_ROW_PAD: u32 = 18;
const SET_CTRL_W: u32 = 96;
/// The appearance segment: its overall width, its inner pad and a chip's height.
///
/// Named rather than written into the painter and the hit test separately —
/// this screen's standing rule, and the class it has paid for three times.
const SEG_W: u32 = 148;
const SEG_PAD: u32 = 3;
const SEG_CHIP_H: u32 = 30;

/// One appearance chip's width, derived so the pair fills the segment.
const fn seg_chip_w() -> u32 {
    (SEG_W - SEG_PAD * 2) / 2
}

/// The content column: the page inset, bounded so the rows do not stretch to a
/// maximised window's width and leave their controls a screen away from their
/// titles.
fn settings_col(region: Rect) -> Rect {
    let w = region.w.saturating_sub(SET_PAD * 2).min(SET_MAX_W);
    Rect::new(SET_PAD, SET_PAD, w, region.h.saturating_sub(SET_PAD * 2))
}

/// How many rows a group holds — the switch groups from [`spec::OPTIONS`], the
/// key group from [`spec::KEY_ROWS`], and appearance is the single theme row.
fn settings_group_rows(group: &str) -> u32 {
    match group {
        "keys" => u(spec::KEY_ROWS.len()),
        "appearance" => 1,
        other => u(spec::OPTIONS.iter().filter(|o| o.group == other).count()),
    }
}

/// The card rectangle a group occupies, region-local.
fn settings_group_rect(region: Rect, group: &str) -> Rect {
    let col = settings_col(region);
    let mut y = col.y;
    for (key, _) in spec::OPTION_GROUPS {
        let rows = settings_group_rows(key);
        y += SET_HEAD_H + SET_HEAD_GAP;
        if key == group {
            return Rect::new(col.x, y, col.w, rows * SET_ROW_H);
        }
        y += rows * SET_ROW_H + SET_GROUP_GAP;
    }
    Rect::new(col.x, y, col.w, 0)
}

/// A row's control seat, at the trailing end.
fn settings_ctrl_rect(row: Rect, w: u32) -> Rect {
    let h = 32;
    Rect::new(
        row.x + row.w.saturating_sub(SET_ROW_PAD + w),
        row.y + (SET_ROW_H.saturating_sub(h)) / 2,
        w,
        h,
    )
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

/// R1697 — the resize mark: two diagonals climbing out of the bottom-right
/// corner, the form every window manager and every reference toolkit uses for
/// a size grip.
fn resize_mark(rect: Rect, ink: Color) -> Scene {
    let (w, h) = (rect.w, rect.h);
    strokes(
        rect,
        &[
            vec![(w - 11, h - 3), (w - 3, h - 11)],
            vec![(w - 6, h - 3), (w - 3, h - 6)],
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
///
/// ★ R1697 — `restore` is the maximise control's OTHER face. The control
/// toggles, and a control that toggles without changing its mark tells a person
/// the same thing in both states — which is the shape R1690 named: a capability
/// that exists and is not drawn is one nobody can use.
fn affordance_mark(
    affordance: CardAffordance,
    rect: Rect,
    ink: Color,
    restore: bool,
) -> Vec<Scene> {
    let (cx, cy) = (rect.w / 2, rect.h / 2);
    match affordance {
        CardAffordance::Settings => (0..3)
            .map(|n| dot(cx - 1, cy - 5 + n * 5, 2, ink))
            .collect(),
        CardAffordance::TearOff => vec![detach_mark(rect, ink)],
        // Two overlapping squares, the form a restore control has everywhere:
        // one box come back out of another.
        CardAffordance::Maximize if restore => vec![strokes(
            rect,
            &[
                vec![
                    (cx - 6, cy - 2),
                    (cx + 2, cy - 2),
                    (cx + 2, cy + 6),
                    (cx - 6, cy + 6),
                    (cx - 6, cy - 2),
                ],
                vec![(cx - 2, cy - 6), (cx + 6, cy - 6), (cx + 6, cy + 2)],
            ],
            ink,
            1,
        )],
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
    children.push(search_chip(state, palette, searching));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("shell.appbar")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(Rect::new(0, 0, win_w(), APP_BAR_H))),
        ),
        "shell.appbar",
        &state.at(),
    )
}

/// The application bar's search field: the hint when empty, the query when not,
/// and a caret while it has the keys.
///
/// Split out at R1696 because the bar grew a line past the hundred this project
/// allows a function, and the search field is the part of it with its own
/// three-way state — which is the one worth having a name.
fn search_chip(state: &ShellState, palette: Palette, searching: bool) -> Scene {
    let search = state.search.get();
    Scene::Container(
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
    )
}

fn sub_bar_scene(state: &ShellState, palette: Palette) -> Scene {
    let placed = state.placed().len();
    let preset = SubChip::Preset.rect();
    let children = vec![
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
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("shell.subbar")
                .with_style(BoxStyle::filled(palette.canvas))
                .with_layout(absolute(Rect::new(
                    RAIL_W,
                    APP_BAR_H,
                    win_w() - RAIL_W - PALETTE_W,
                    SUB_BAR_H,
                ))),
        ),
        "shell.subbar",
        &state.at(),
    )
}

/// The saved-layout menu: a **top-level popup**, painted in window space — the
/// same space [`preset_item_rect`] gives the hit test.
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
    let nav = state.at();
    for (n, seat) in spec::RAIL.iter().enumerate() {
        let key = seat.key;
        let rect = rail_rect(u(n));
        let on = nav == key;
        let ink = if on { palette.accent_fg } else { palette.muted };
        // R1668 — a seat this application cannot take you to is DECLARED
        // unavailable rather than painted grey by hand. The declaration is what
        // makes it inert to the pointer, fades its ink, announces it to a screen
        // reader and puts the reason on `scene/disabled`; a hand-picked grey
        // would do only the last of those, in a way nothing can check.
        //
        // ★ R1695 — the reason comes from the ROSTER, so the seat's paint, its
        // refusal and its accessibility node are one fact. Three of these were
        // painted live and refused nothing until this round.
        let layout = state
            .roster
            .get(key)
            .and_then(|d| d.standing.why())
            .map_or_else(
                || absolute(rect),
                |why| absolute(rect).with_unavailable(why.clone()),
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
            ACCOUNT_INITIALS,
            Rect::new(8, 9, 24, 14),
            FONT_TINY,
            palette.on_accent,
        )])
        .with_tag("shell.rail.account")
        .with_style(BoxStyle::filled(palette.accent).with_corner_radius(16))
        .with_layout(absolute(Rect::new(10, win_h() - APP_BAR_H - 46, 32, 32))),
    ));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(entries)
                .with_tag("shell.rail")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(Rect::new(
                    0,
                    APP_BAR_H,
                    RAIL_W,
                    win_h().saturating_sub(APP_BAR_H),
                ))),
        ),
        "shell.rail",
        &nav,
    )
}

/// The Settings destination's page, region-local.
///
/// ★★ R1695 — the second page, and what makes the region worth building: a
/// paged region with one page proves nothing about paging.
fn settings_scene(state: &ShellState, palette: Palette, region: Rect) -> Vec<Scene> {
    let col = settings_col(region);
    let mut out = Vec::new();
    for (key, heading) in spec::OPTION_GROUPS {
        let card = settings_group_rect(region, key);
        out.push(label(
            heading,
            Rect::new(col.x, card.y - SET_HEAD_H - SET_HEAD_GAP, col.w, SET_HEAD_H),
            FONT_SMALL,
            palette.muted,
        ));
        let rows = match key {
            "keys" => settings_key_rows(palette, region),
            "appearance" => settings_theme_row(state, palette, region),
            group => settings_option_rows(state, palette, region, group),
        };
        out.push(Scene::Container(
            ContainerNode::new(rows)
                .with_tag(format!("shell.settings.group.{key}"))
                .with_style(
                    BoxStyle::filled(palette.panel)
                        .with_corner_radius(12)
                        .with_border(Border::new(palette.outline, 1)),
                )
                .with_layout(absolute(card)),
        ));
    }
    out
}

/// A row's title and the sentence under it, in the card's own space.
///
/// Silenced as `part_of` the control it names: the switch beside it takes its
/// accessible name from this text, so a reader who heard both would hear the
/// row twice. `named` is that control's **tag** — a silence that points at
/// prose is a silence pointing at nothing, which is what `dangling` counts and
/// what the first draft of this page produced seven of.
fn settings_text(
    key: &str,
    title: &str,
    gist: &str,
    row: Rect,
    palette: Palette,
    named: String,
) -> Scene {
    let inner = Rect::new(
        SET_ROW_PAD,
        row.y,
        row.w.saturating_sub(SET_ROW_PAD * 2),
        row.h,
    );
    Scene::Container(
        ContainerNode::new(vec![
            label(
                title,
                Rect::new(0, 15, inner.w.saturating_sub(SET_CTRL_W), 16),
                FONT_TITLE,
                palette.ink,
            ),
            label(
                gist,
                Rect::new(0, 34, inner.w.saturating_sub(SET_CTRL_W), 15),
                FONT_BODY,
                palette.muted,
            ),
        ])
        .with_tag(format!("shell.settings.row.{key}"))
        .with_layout(absolute(inner)),
    )
    .silenced(Silence::part_of(named))
}

/// The switch rows of one group.
fn settings_option_rows(
    state: &ShellState,
    palette: Palette,
    region: Rect,
    group: &str,
) -> Vec<Scene> {
    let on = state.options.get();
    let theme = use_theme(THEME_TAG).theme_animated();
    let card = settings_group_rect(region, group);
    let mut out = Vec::new();
    // Two indices, and both are load-bearing: `n` is the row's place in this
    // group's card and `index` its place in the specification, which is what the
    // value array is keyed by. Collapsing them would work only while every group
    // held every option.
    for (n, (index, option)) in spec::OPTIONS
        .iter()
        .enumerate()
        .filter(|(_, option)| option.group == group)
        .enumerate()
    {
        let row = Rect::new(0, u(n) * SET_ROW_H, card.w, SET_ROW_H);
        out.push(settings_text(
            option.key,
            option.title,
            option.gist,
            row,
            palette,
            format!("shell.settings.option.{}", option.key),
        ));
        let seat = settings_ctrl_rect(row, 64);
        out.push(Scene::Container(
            ContainerNode::new(vec![switch::view_switch(
                format!("shell.settings.option.{}", option.key),
                ToggleState::Idle,
                on[index],
                &theme,
                &SwitchStyle::m3(),
                option.title,
            )])
            .with_layout(absolute(seat)),
        ));
    }
    out
}

/// The two key rows, whose affordance is booked for a later release.
fn settings_key_rows(palette: Palette, region: Rect) -> Vec<Scene> {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut out = Vec::new();
    for (n, key_row) in spec::KEY_ROWS.iter().enumerate() {
        let card = settings_group_rect(region, "keys");
        let row = Rect::new(0, u(n) * SET_ROW_H, card.w, SET_ROW_H);
        out.push(settings_text(
            key_row.key,
            key_row.title,
            key_row.gist,
            row,
            palette,
            format!("shell.settings.key.{}", key_row.key),
        ));
        let seat = settings_ctrl_rect(row, SET_CTRL_W);
        // ★★ R1695 — the framework's BUTTON, not a box with a word in it.
        // Hand-rolling it is the class R1673 measured on the sibling screen,
        // where a switch was drawn as a track with no knob at all; here the
        // hand-rolled version put its label flush against the left border,
        // because centring a label is what a button does and a box does not.
        // Measured on the first draft by ink span: 683 in a box from 682.
        out.push(Scene::Container(
            ContainerNode::new(vec![button::view_button(
                key_row.verb,
                ButtonState::Disabled,
                0.0,
                &ButtonColors::filled_tonal(&theme),
                &ButtonStyle::m3_default(format!("shell.settings.key.{}", key_row.key))
                    .with_corner_radius(8)
                    .with_size(Size::px(seat.w, seat.h))
                    .with_label_font_size_px(FONT_BODY)
                    // ★ R1696 — booked for a later release, so not a Tab stop.
                    // The flag is NOT what makes that true and a counterfactual
                    // proved it: flipping this to `true` changed nothing,
                    // because the row below declares `Unavailable` and R1554's
                    // enumeration returns at a disabled region before it reads
                    // any child's flag (`r1554_a_disabled_region_contributes_
                    // no_tab_stops`). It stays `false` so the declaration and
                    // the guarantee agree, and this comment says which one is
                    // load-bearing — a widget default of `true` under a
                    // structural `no` reads as a control that is one edit away
                    // from being reachable.
                    .with_focusable(false),
            )])
            .with_layout(
                absolute(seat).with_unavailable(Unavailable::reserved(key_row.reserved_for)),
            ),
        ));
    }
    out
}

/// The appearance row: one two-way segment over the theme.
fn settings_theme_row(state: &ShellState, palette: Palette, region: Rect) -> Vec<Scene> {
    let card = settings_group_rect(region, "appearance");
    let row = Rect::new(0, 0, card.w, SET_ROW_H);
    let dark = theme_word(&state.theme) == "dark";
    let theme = use_theme(THEME_TAG).theme_animated();
    let seg_w = SEG_W;
    let seat = settings_ctrl_rect(row, seg_w);
    let mut segs = Vec::new();
    for (n, name) in spec::THEMES.iter().enumerate() {
        let on = (n == 0) == dark;
        let w = seg_chip_w();
        // The chosen half is the accent surface and the other is the tonal
        // one — the reference's own two-way appearance segment, painted with
        // the catalogue's button rather than a box and a word.
        let colors = if on {
            ButtonColors::accent(&theme)
        } else {
            ButtonColors::filled_tonal(&theme)
        };
        segs.push(Scene::Container(
            ContainerNode::new(vec![button::view_button(
                name,
                ButtonState::Idle,
                0.0,
                &colors,
                &ButtonStyle::m3_default(format!("shell.settings.theme.{n}"))
                    .with_corner_radius(6)
                    .with_size(Size::px(w, SEG_CHIP_H))
                    .with_label_font_size_px(FONT_BODY),
            )])
            .with_layout(absolute(Rect::new(
                SEG_PAD + u(n) * w,
                SEG_PAD,
                w,
                SEG_CHIP_H,
            ))),
        ));
    }
    vec![
        settings_text(
            "theme",
            spec::THEME_ROW.0,
            spec::THEME_ROW.1,
            row,
            palette,
            // The row names the pair, not either button, so it folds into the
            // group that owns them.
            "shell.settings.group.appearance".to_owned(),
        ),
        Scene::Container(
            ContainerNode::new(segs)
                .with_tag("shell.settings.theme")
                .with_style(BoxStyle::filled(palette.canvas).with_corner_radius(8))
                .with_layout(absolute(Rect::new(
                    seat.x,
                    seat.y + 1,
                    seg_w,
                    SEG_CHIP_H + SEG_PAD * 2,
                ))),
        ),
    ]
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
fn header_scene(card: &Card, rect: Rect, palette: Palette, maximized: bool) -> Vec<Scene> {
    /// The clearance the affordance strip keeps at the header's right edge.
    const HDR_TAIL: u32 = 6;
    /// The narrowest a title may be and still be worth painting.
    const MIN_TITLE: u32 = 24;
    /// The ready badge: its dot, its word, and the gap before it.
    const BADGE_W: u32 = 54;

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
    // ★★ R1672 — the header GIVES WAY in a stated order, and a part that does
    // not fit is **not painted** rather than clamped.
    //
    // It used to be one expression — a title width with `.max(40)` on the end —
    // and everything after it was measured from that clamped number, so a card
    // shrunk to one board cell (75px) painted its title 11px past its own
    // frame, its ready dot 21px past, the word `LIVE` **65px** past, and two
    // affordance slots off its left edge. Twenty-five marks in a state the
    // sweep already ran, and nothing could see them: the same shape R1668 named
    // — one fact, two clamps, and the second one arrives too late to be told.
    //
    // The order below is the judgement, and it is the one a toolbar makes:
    //
    // 1. the grip and the kind dot are the card's identity and never give way;
    // 2. the affordance strip keeps as many slots as fit, dropping from the
    //    LEFT so the last-declared stays nearest the edge a hand reaches for;
    // 3. the ready badge goes before the title does;
    // 4. the title takes what is left, and elides inside it.
    let offered = card.chrome().offered();
    let text_x = grip.x + grip.w + 20;
    let right = rect.x + rect.w;
    // What the header can give the strip once the identity and a title that
    // says something are paid for. Derived, so the count the paint walks and
    // the width the strip is sized from are one number.
    let shown = usize::min(
        offered.len(),
        (right.saturating_sub(text_x + MIN_TITLE + HDR_TAIL) / SLOT_W) as usize,
    );
    let dropped = offered.len() - shown;
    let text_room = right.saturating_sub(text_x + u(shown) * SLOT_W + HDR_TAIL);
    let show_badge = card.state().is_ready() && text_room >= BADGE_W + MIN_TITLE;
    let title_w = if show_badge {
        text_room - BADGE_W
    } else {
        text_room
    };
    if title_w > 0 {
        out.push(label(
            card.title(),
            Rect::new(text_x, rect.y + 9, title_w, 16),
            FONT_BODY,
            palette.ink,
        ));
    }
    if show_badge {
        let badge_x = text_x + title_w + 4;
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
    for (n, affordance) in offered.iter().enumerate().skip(dropped) {
        let slot = affordance_rect(rect, u(offered.len()), u(n));
        out.push(Scene::Container(
            ContainerNode::new(affordance_mark(
                *affordance,
                local(slot),
                palette.muted,
                maximized,
            ))
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

// ★★★ R1695 — there is no `centred` helper here, and the reason is a
// measurement worth keeping.
//
// The first repair for the left-flush button labels set
// `TextStyle::with_align(TextAlign::Center)`. Measured by ink span on the
// rendered page it did **nothing**: the segment's `Dark` inked 634..659 inside a
// chip starting at 635, and `Import…` inked 683..729 in a box from 682 — the
// glyphs at the node's left edge in every case. The framework's own button does
// not use that property either; `pinion_widget_paint::button::view_button`
// centres with `JustifyContent::Center` on a flex row, which is the idiom this
// screen now uses. Filed as `debt-a-declared-text-alignment-does-nothing-on-an-
// absolutely-placed-run` rather than worked around, because the property is
// published, accepted and reported back on `scene/snapshot`.

/// ★★★★★ R1694 — [`clipped`], **addressable**.
///
/// A table cell painted without a tag is a value a reader can see and cannot
/// ask about: the row is one box and its cells are its siblings, so the whole
/// row collapses to one run of words. Measured at 6.11.1, a model-driven item
/// view answers a cell query with the cell's own name, its row, its column and
/// its column header — the strong case, and the reason both of this screen's
/// tables are announced cell by cell rather than row by row.
fn cell(tag: String, text: &str, rect: Rect, px: u32, fg: Color, overflow: TextOverflow) -> Scene {
    text_run(
        tag,
        text,
        rect,
        TextStyle::new()
            .with_size_px(px)
            .with_fg(fg)
            .with_overflow(overflow),
    )
}

/// The tag a table card's cell is addressed by.
fn cell_tag(id: &str, row: usize, column: usize) -> String {
    format!("card.{id}.cell.{row}_{column}")
}

/// The tag a table card's column header is addressed by.
fn head_cell_tag(id: &str, column: usize) -> String {
    format!("card.{id}.head.{column}")
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
                .enumerate()
                .map(|(c, (name, x, w))| {
                    cell(
                        head_cell_tag(id, c),
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
            .enumerate()
            .map(|(c, ((column, x, w), value))| {
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
                cell(
                    cell_tag(id, n, c),
                    value,
                    Rect::new(*x, 3, *w, 13),
                    FONT_TINY,
                    ink,
                    overflow,
                )
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
    // `row` is `None` for the header strip and the row index otherwise, which is
    // what the cell tags are built from.
    let cells = |ink: Color, cols: [&str; 3], warn: bool, row: Option<usize>| {
        let tag = |column: usize| match row {
            None => head_cell_tag(id, column),
            Some(r) => cell_tag(id, r, column),
        };
        let mut out = vec![cell(
            tag(0),
            cols[0],
            Rect::new(12, 2, ID_W, 13),
            FONT_TINY,
            if warn { palette.warn } else { ink },
            TextOverflow::Ellipsis,
        )];
        if path_w > 0 {
            out.push(cell(
                tag(1),
                cols[1],
                Rect::new(12 + ID_W + 6, 2, path_w, 13),
                FONT_TINY,
                ink,
                TextOverflow::EllipsisStart,
            ));
        }
        if with_seen {
            out.push(cell(
                tag(2),
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
            None,
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
                Some(n),
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
                // The plot area and its stroke are HOW the region is drawn; the
                // region itself states the series, so these two are declared
                // quiet rather than left undecided.
                Sparkline::new(MATCH_SERIES.to_vec())
                    .with_color(kind_color("filter"))
                    .with_tag_prefix("match.spark")
                    .build(
                        Rect::new(0, 0, area.w, card.y + card.h - spark_y),
                        &ChartStyle::default(),
                    )
                    .silenced(Silence::part_of(format!("card.{id}.sparkline"))),
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
        code_chip(
            def.map_or("", |d| d.code),
            Rect::new(rect.x + 12, rect.y + 10, 40, 32),
            BoxStyle::filled(kind_color(kind)).with_corner_radius(6),
            palette.on_accent,
            Some(format!("card.{id}.code")),
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

/// Declare that `node` occupies its parent's chrome band of `role`. R1674.
///
/// One place, because the claim has to reach every mark in a band and a band is
/// several siblings — a grip, a title, a status pill, two buttons. A per-call
/// `.with_layout(...)` at each of those sites is five chances to miss one, and
/// a missed one reports as a mark that landed on the header rather than as a
/// mark that IS the header.
fn in_chrome(node: Scene, role: ChromeRole) -> Scene {
    let mut node = node;
    if let Some(layout) = node.layout_style_mut() {
        *layout = layout.clone().with_chrome_slot(role);
    }
    node
}

/// ★★ R1696 — a region is a keyboard stop **because the specification says so**.
///
/// The declaration is read rather than restated: `spec::FOCUS_RING` is the ring,
/// and this is the only place the paint consults it, so a stop added to the
/// table appears on screen and a stop removed from it disappears. Writing
/// `.with_focusable(true)` at each painter would be five independent decisions
/// and the table would become a description of what somebody remembered.
fn keyboard_stop(node: Scene, tag: &str, at: &str) -> Scene {
    let declared = spec::FOCUS_RING
        .iter()
        .any(|stop| stop.tag == tag && stop.at.shows_at(at));
    node.with_focusable(declared)
}

fn card_scene(
    card: &Card,
    rect: Rect,
    selected: bool,
    editing: bool,
    cell: (u32, u32),
    palette: Palette,
    maximized: bool,
) -> Scene {
    let inside = local(rect);
    // ★★ R1674 — the header's marks say they are IN the header band, so the
    // containment check judges them against that band and not against the
    // card's content rectangle. Both halves are needed and neither works
    // alone: without the card's `Chrome::header` declaration the band does not
    // exist to be judged against, and without these claims declaring it would
    // report the card's own title as an escape.
    //
    // What this buys, beyond silence: a mark that lands on the title strip
    // WITHOUT claiming it now arrives as `trespass: ["chrome:header"]` instead
    // of as an undifferentiated overhang, so a reader is told which band was
    // invaded. The floor has no form for that — its whole reservation is four
    // integers with the reason discarded.
    let mut children: Vec<Scene> = header_scene(card, header_rect(inside), palette, maximized)
        .into_iter()
        .map(|node| in_chrome(node, ChromeRole::Header))
        .collect();
    children.extend(body_scene(card, body_rect(inside, editing), palette));
    if editing {
        children.push(in_chrome(
            edit_bar_scene(card.id().as_str(), edit_bar_rect(inside), cell, palette),
            ChromeRole::Footer,
        ));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!("card.{}", card.id().as_str()))
            .with_style(card_style(palette, selected, editing))
            .with_layout(absolute(rect)),
    )
}

/// A card's skin: its fill, its outline, and **the bands of itself it keeps**.
///
/// ★★ R1674 — the header strip and the edit bar are now DECLARED chrome
/// ([`pinion_core::style::Chrome`]) rather than two rectangles this file works
/// out privately. The card's content rectangle is therefore the region its rows
/// actually get, and `scene/containment` can say `chrome:header` about a mark
/// that landed on the title strip instead of the undifferentiated "out of
/// bounds" that was the only available answer.
///
/// This is where the floor stops: probed at 6.11, a custom-painted widget can
/// publish the same INSET through a four-integer content-margin setter, and
/// reading it back gives four integers with the reason gone — a caption band
/// and a three pixel border are the same four numbers there. A card header is not one of
/// the ~20 complex controls with their own sub-control enum, so there is no
/// second way to ask either.
fn card_style(palette: Palette, selected: bool, editing: bool) -> BoxStyle {
    // The selection ring: one card is the keyboard's subject and a person has
    // to see which. Accent on the border rather than a different fill, so a
    // selected card that is also failing still reads as failing.
    let border = if selected {
        Border::new(palette.accent_fg, CARD_RING)
    } else {
        Border::new(palette.outline, CARD_OUTLINE)
    };
    // ★★ The band extents are measured from the card's EDGE and `content_of`
    // subtracts the border before them, so each declaration is the distance to
    // the band's far side LESS the border currently drawn.
    //
    // The subtlety is [`CARD_FRAME`], which is deliberately the WIDER of the
    // two borders and constant, so a card's rows do not shift by a pixel the
    // moment a person selects it. The card therefore reserves more than it
    // draws when it is not selected, and a declaration that ignored that would
    // be a pixel out — measured, exactly that: three edit bars reported one
    // pixel above their own footer band, by a check that had just been told
    // where the band was. Deriving both sides from the same two constants is
    // what makes the placement functions and this one incapable of disagreeing,
    // which `r1674_the_declared_bands_are_the_placed_bands` then asserts.
    let mut style = BoxStyle::filled(palette.panel)
        .with_corner_radius(10)
        .with_border(border)
        .with_chrome(Chrome::header(CARD_HDR.saturating_sub(border.width)));
    if editing {
        // The size-stepper strip only exists in layout-edit mode, so the
        // content rectangle is smaller in that mode — a fact about this card
        // that no reader could previously obtain.
        style = style.with_chrome(Chrome::new(
            ChromeEdge::Bottom,
            EDIT_BAR_H + CARD_FRAME - border.width,
            ChromeRole::Footer,
        ));
    }
    style
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
    // ★★ R1697 — the corner, painted last so it is over the body it sits in,
    // and from the SAME function the hit test reads. An affordance a person is
    // meant to grab has to be visible: R1690's lesson is that a capability
    // built and not drawn is one no gate can see and nobody can use.
    let grip = Rect::new(
        inside.w.saturating_sub(FLOAT_GRIP),
        inside.h.saturating_sub(FLOAT_GRIP),
        FLOAT_GRIP,
        FLOAT_GRIP,
    );
    children.push(Scene::Container(
        ContainerNode::new(vec![resize_mark(local(grip), palette.muted)])
            .with_tag(format!("float.{}.resize", float.id))
            .with_layout(absolute(grip)),
    ));
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
                    seat.reserved_for()
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

/// R1697 — the operations table, on the wire.
///
/// Each row carries the introspection slot that must move once the operation
/// has run, which is what turns the table from a list of names into something
/// a client can check for itself: read the slot, cause the operation the way
/// the row says it can be caused, read the slot again.
fn operations_json() -> serde_json::Value {
    serde_json::Value::Array(
        spec::OPERATIONS
            .iter()
            .map(|op| {
                serde_json::json!({
                    "name": op.name,
                    "verb": op.verb.map(|(action, arg)| serde_json::json!([action, arg])),
                    "gesture": op.gesture,
                    "witness": op.witness,
                    "needs": op.needs,
                })
            })
            .collect(),
    )
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
        // ★ R1695 — `open` joined `reserved_for`, because a seat that is neither
        // this page nor booked for a release is a third thing and the old two
        // columns could not say it.
        "rail": spec::RAIL.iter().map(|seat| serde_json::json!({
            "key": seat.key,
            "title": seat.title,
            "reserved_for": seat.reserved_for(),
            "open": matches!(seat.seat, spec::Seat::Page),
        })).collect::<Vec<_>>(),
        "rail_active": spec::RAIL_ACTIVE,
        // ★ R1695 — the Settings destination.
        "options": spec::OPTIONS.iter().map(|o| serde_json::json!({
            "key": o.key, "title": o.title, "gist": o.gist,
            "group": o.group, "opens": o.opens,
        })).collect::<Vec<_>>(),
        "key_rows": spec::KEY_ROWS.iter().map(|r| serde_json::json!({
            "key": r.key, "title": r.title, "gist": r.gist,
            "verb": r.verb, "reserved_for": r.reserved_for,
        })).collect::<Vec<_>>(),
        "option_groups": spec::OPTION_GROUPS.iter().map(|(key, title)| serde_json::json!({
            "key": key, "title": title,
        })).collect::<Vec<_>>(),
        "themes": spec::THEMES,
        "theme_row": { "title": spec::THEME_ROW.0, "gist": spec::THEME_ROW.1 },
        // ★★ R1696 — where the Tab key stops, in the order it stops there, and
        // WHAT each stop holds. The last column is the part a tag cannot carry:
        // an agent reading this learns that landing on `shell.rail` puts it
        // among the tool's destinations rather than on one of them.
        "focus_ring": spec::FOCUS_RING.iter().map(|stop| serde_json::json!({
            "tag": stop.tag, "holds": stop.holds, "at": where_word(stop.at),
        })).collect::<Vec<_>>(),
        // ★★ R1697 — what this screen can be ASKED to do, published so an
        // agent reads the operations rather than discovering them by trying.
        "operations": operations_json(),
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
        // ★ R1694 — what reaches a reader who never sees the drawing, EXPANDED
        // here rather than listed, so a family that grows a member cannot be
        // satisfied by the members that were there when the table was written.
        // ★ R1695 — each row now says WHICH destination it belongs to, so a
        // client can ask the census about a page it is not looking at.
        "voices": spec::VOICES.iter().flat_map(|voice| {
            voice.population.members().into_iter().map(move |member| serde_json::json!({
                "tag": voice.tag.replace("{}", &member),
                "role": voice.role,
                "at": where_word(voice.at),
            }))
        }).collect::<Vec<_>>(),
        "silences": spec::SILENCES.iter().flat_map(|(tag, population, kind, at)| {
            population.members().into_iter().map(move |member| serde_json::json!({
                "tag": tag.replace("{}", &member),
                "kind": kind,
                "at": where_word(*at),
            }))
        }).collect::<Vec<_>>(),
        "locked": spec::LOCKED.iter().flat_map(|(tag, population, at)| {
            population.members().into_iter().map(move |member| serde_json::json!({
                "tag": tag.replace("{}", &member),
                "at": where_word(*at),
            }))
        }).collect::<Vec<_>>(),
    })
}

/// ★ R1695 — the Settings switches, as the wire reads them.
fn options_json(state: &ShellState) -> serde_json::Value {
    let on = state.options.get();
    serde_json::Value::Array(
        spec::OPTIONS
            .iter()
            .zip(on)
            .map(|(option, on)| {
                serde_json::json!({
                    "key": option.key,
                    "title": option.title,
                    "gist": option.gist,
                    "group": option.group,
                    "on": on,
                })
            })
            .collect(),
    )
}

/// The wire spelling of where a region belongs — `"*"` for chrome, which is on
/// screen at every destination, and the key otherwise.
const fn where_word(at: spec::Where) -> &'static str {
    match at {
        spec::Where::Chrome => "*",
        spec::Where::At(key) => key,
    }
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
            code_chip(
                def.code,
                Rect::new(8, 7, 32, 32),
                BoxStyle::filled(kind_color(def.kind)).with_corner_radius(8),
                palette.on_accent,
                None,
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
    for row in palette_rows() {
        match row.def {
            // The heading is the group a reader descends through, so it is
            // addressable rather than loose ink between the entries.
            None => children.push(cell(
                format!("shell.palette.section.{}", row.section),
                row.title,
                row.rect,
                FONT_TINY,
                palette.muted,
                TextOverflow::Ellipsis,
            )),
            Some(def) => children.push(palette_row(state, def, row.rect, palette)),
        }
    }
    // Both counts, because the screen's whole claim is the relation between
    // them: this release places four, and holds nine seats open.
    children.push(cell(
        "shell.palette.placed".to_owned(),
        &format!(
            "{} placed of {}",
            state.placed().len(),
            spec::placeable_count()
        ),
        Rect::new(16, panel.h.saturating_sub(30), 130, 16),
        FONT_SMALL,
        palette.muted,
        TextOverflow::Ellipsis,
    ));
    children.push(cell(
        "shell.palette.reserved".to_owned(),
        &format!("{} reserved", spec::reserved_count()),
        Rect::new(
            panel.w.saturating_sub(110),
            panel.h.saturating_sub(30),
            94,
            16,
        ),
        FONT_SMALL,
        palette.muted,
        TextOverflow::Ellipsis,
    ));
    keyboard_stop(
        Scene::Container(
            ContainerNode::new(children)
                .with_tag("shell.palette")
                .with_style(BoxStyle::filled(palette.panel))
                .with_layout(absolute(panel)),
        ),
        "shell.palette",
        &state.at(),
    )
}

/// The toast: what just happened, floating at the foot of the canvas.
fn toast_scene(state: &ShellState, palette: Palette) -> Scene {
    let canvas = canvas_rect();
    let rect = Rect::new(canvas.x + 24, win_h() - 58, 560, 34);
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

/// Every colour the painters here read, resolved from one theme.
///
/// Extracted at its second consumer (R1674): the geometry tests need a palette
/// to build a card's style with, and the alternative — a second literal struct
/// in the test — is a copy that stops resembling the real one the first time a
/// role is added.
fn palette_of(theme: &Theme, dark: bool) -> Palette {
    Palette {
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
    }
}

fn view(_state: (), _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let state = use_shell_state();
    let dark = theme_word(&state.theme) == "dark";
    let palette = palette_of(&theme, dark);

    let journey = state.journey.get();
    let here = journey.here(&state.roster).clone();
    let region = page_rect(here.key.as_ref());
    // ★★★★★ R1695 — the page a destination gets, built by the framework's
    // region so that the pages it is NOT at are never constructed. Before this
    // the rail moved a string and the window did not change: measured through
    // the router, four of seven seats left the screen at 193 painted regions
    // before and 193 after.
    let page = keyboard_stop(
        view_page_region(
            "shell.canvas",
            region,
            palette.canvas,
            &here,
            |here| match here.key.as_ref() {
                "settings" => settings_scene(&state, palette, region),
                _ => dashboard_scene(&state, palette),
            },
        ),
        // ★ R1696 — a stop at the DASHBOARD only, where this region is the
        // board and the arrows already move a selection among its cards. At
        // Settings the same rectangle is a page whose controls are their own
        // stops, and a landmark is not a Tab stop.
        "shell.canvas",
        here.key.as_ref(),
    );

    let children = std::iter::once(page)
        .chain([
            app_bar_scene(&state, palette),
            rail_scene(&state, palette),
            toast_scene(&state, palette),
        ])
        // ★ R1695 — the layout bar and the palette are the DASHBOARD's. They sit
        // outside the region because they sit outside its rectangle, so the
        // substrate's guarantee does not reach them; the specification's
        // destination column is what checks them, in both directions.
        .chain(if spec::shows_board_chrome(here.key.as_ref()) {
            vec![
                sub_bar_scene(&state, palette),
                palette_scene(&state, palette),
            ]
        } else {
            Vec::new()
        })
        .chain([
            // ★★ R1672 — the preset menu is a POPUP: anchored to the sub bar's
            // chip, bounded by the window. It used to be a child of the bar and
            // hung 81 pixels below it, which is an escape and was invisible
            // until the ink gate reached this screen. A sibling here also puts
            // it over everything it opens across, which a child of one bar can
            // never be.
            if state.preset_open.get() && spec::shows_board_chrome(here.key.as_ref()) {
                preset_menu_scene(&state, palette)
            } else {
                Scene::Container(ContainerNode::new(Vec::new()))
            },
            label(
                HELP_STRIP,
                Rect::new(canvas_rect().x + 610, win_h() - 47, 470, 14),
                FONT_SMALL,
                palette.muted,
            ),
        ])
        .collect::<Vec<_>>();

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(palette.canvas))
            .with_layout(LayoutStyle::new().with_size(Size::px(win_w(), win_h()))),
    )
}

/// The dashboard destination's page, region-local: the board, and the cards
/// torn off it.
fn dashboard_scene(state: &ShellState, palette: Palette) -> Vec<Scene> {
    let board = state.board.get();
    let selected = state.selected.get();
    let editing = state.editing.get();
    let drag = state.drag.get();
    // R1697 — which card, if any, wears the restore face of the maximise
    // control. Read once here rather than per card so the paint and the toggle
    // read one fact.
    let maximized = state.maximized.get();

    let canvas = canvas_rect();
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
            maximized.as_ref().is_some_and(|m| m.id() == card.id()),
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
    let mut canvas_children = vec![
        scroll_pane(
            &state.canvas_scroll,
            Rect::new(0, 0, canvas.w, canvas.h),
            (0, GAP),
            // Every press goes to the one root `External` that runs this
            // screen's own hit test, so the pane must be invisible to the
            // router (R1655).
            PanePointer::PassesThrough,
            canvas_children,
        )
        // The viewport is a clip rather than a thing on the screen: what a
        // reader walks is the board inside it.
        .silenced(Silence::layout("the board's scrolling viewport")),
    ];
    // ★ R1697 — back to front, which is the REVERSE of the order the hit test
    // walks. Both read `floats_front_to_back`, so painting the frontmost panel
    // last and hitting it first are one decision rather than two that agree
    // until somebody changes one of them.
    for float in state.floats_front_to_back().iter().rev() {
        if let Some(scene) = float_scene(state, float, palette) {
            canvas_children.push(scene);
        }
    }
    canvas_children
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

    /// ★★★★★ R1698 — **the hook this screen did not have, so its keyboard was
    /// not reachable from a keyboard.**
    ///
    /// Measured before it existed, by driving the running application: the wire
    /// `invoke("key", "ArrowRight")` moved the board's selection and a REAL
    /// `scene/key` press moved nothing at all. The screen publishes a keymap of
    /// twelve chords through `KEYMAP` and not one of them was reachable by
    /// pressing a key. Every test that drove the keyboard passed because every
    /// test drove it through the wire — R1693's lesson, on the sibling screen,
    /// recurring here: *the test and the defect were the same mistake.*
    ///
    /// The census across all 225 examples: 172 bindings implement this hook and
    /// 135 of them read `focused`. This screen implemented it zero times.
    ///
    /// `focused` is threaded through rather than dropped, which is what makes
    /// the composite cursors work at all: the arrows have to mean something
    /// different depending on which composite the reader is standing in, and
    /// three of this tree's bindings forward a key to one External without ever
    /// asking where focus is.
    fn apply_key(
        _scene: &mut Scene,
        focused: Option<&str>,
        chord: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        ShellOracle::key_at(&use_shell_state(), focused, chord)
    }
}

impl WidgetA11y for AnalyzerShellView {
    /// ★★★★★ R1698 — **where the cursor rests inside the focused composite.**
    ///
    /// WAI-ARIA's composite model in two lines: the AT focus stays on the
    /// composite and `aria-activedescendant` names the member the arrows are
    /// on. This screen returned nothing at all before, so an assistive
    /// technology landing on the rail was told "Destinations" and never which
    /// destination the cursor was on — and the framework's focus ring framed
    /// the whole bar rather than the member.
    ///
    /// The ring comes free with it: `resolve_focus_ring_tag` reads exactly this
    /// hook, so publishing the descendant is also what makes the cursor visible
    /// on screen.
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        let stop = focused?;
        let state = use_shell_state();
        let cursor = state
            .cursor_of(stop)
            // ★★★★★ R1699 — the INNERMOST tag, not the member at this level.
            // ARIA's `aria-activedescendant` addresses any descendant of the
            // element owning the Tab stop, and the framework's focus ring reads
            // this same hook, so a cursor that has gone into the tab list frames
            // the tab rather than the list.
            .and_then(|roving| roving.active_descendant().map(str::to_owned))
            // The board's cursor is its selection: it is spatial rather than a
            // linear roster, so it declares no `Roving` and reports the card it
            // is on. It has had that cursor since R1662 and published it to
            // nobody.
            .or_else(|| {
                (stop == "shell.canvas")
                    .then(|| state.selected.get().map(|id| format!("card.{id}")))
                    .flatten()
            });
        Some(AccessFocus::addressing(stop, cursor))
    }

    /// ★★★★★ R1694 — **the screen a reader can walk, locked seats included.**
    ///
    /// Before this round the dashboard painted 128 addressable regions and
    /// announced five: a group for the window and one per card, each holding
    /// nothing. The rail, both bars, two tables, the decode tree, seventy-two
    /// bytes and the whole palette were not in the tree at all — and with them
    /// went the screen's own claim, which is that **nine seats are locked and
    /// each says what it is booked under**. The framework has computed that
    /// reason since R1668 and published it on `scene/disabled`; none of the
    /// eleven locked regions had a node to carry it.
    ///
    /// Measured at 6.11.1 by building and running the same shape: a locked entry
    /// in an item view and a locked destination in a tab bar answer
    /// `focusable, selectable` and carry **no unavailable state at all** — the
    /// bit survives only on a plain widget — so a reader there is invited to
    /// activate exactly the seats the screen has closed. Here every locked seat
    /// is announced unavailable, keeps its place in the set, and carries the
    /// kind, the detail and the recourse the bit cannot hold.
    /// ★★ R1695 — and the tree now follows the rail. A destination's nodes are
    /// emitted only where that destination is showing, which is the same
    /// property the paint has and for the same reason: a reader offered a
    /// control that is not on screen is offered a control nobody can reach.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_shell_state();
        let journey = state.journey.get();
        let here = journey.here(&state.roster);
        let dashboard = spec::shows_board_chrome(here.key.as_ref());
        let mut root = AccessNode::new(VIEW_TAG, AriaRole::Group)
            .with_name("Analyzer dashboard")
            .with_value(AccessValue::Text(format!(
                "{} of {} widgets placed on layout \"{}\", {} reserved, source {}",
                state.placed().len(),
                spec::placeable_count(),
                state.preset.get(),
                spec::reserved_count(),
                state.source.get(),
            )))
            .with_child("shell.appbar")
            .with_child("shell.rail");
        if dashboard {
            root = root.with_child("shell.subbar");
        }
        root = root.with_child("shell.canvas");
        if dashboard {
            root = root.with_child("shell.palette");
        }
        let mut nodes = vec![root.with_child("shell.toast")];
        nodes.extend(app_bar_nodes(&state));
        nodes.extend(rail_nodes(&state));
        // The region says which destination arrived — the fact the reference
        // toolkit's paged container leaves empty on its own accessible value.
        let mut region = page_region_node("shell.canvas", here);
        if dashboard {
            let (value, children, cards) = board_nodes(&state);
            region = region.with_value(AccessValue::Text(value));
            for child in children {
                region = region.with_child(child);
            }
            nodes.push(region);
            nodes.extend(sub_bar_nodes(&state));
            nodes.extend(cards);
            nodes.extend(palette_nodes(&state));
        } else {
            let (children, rows) = settings_nodes(&state);
            for child in children {
                region = region.with_child(child);
            }
            nodes.push(region);
            nodes.extend(rows);
        }
        nodes.push(
            AccessNode::new("shell.toast", AriaRole::Status)
                .with_name("Activity")
                .with_value(AccessValue::Text(state.toast.get()))
                .with_live(AccessLive::Polite),
        );
        nodes
    }
}

/// The application bar: which view is open, what capture is being read, how fast
/// it is arriving, and the search field.
fn app_bar_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let current = state.tab.get();
    let searching = state.searching.get();
    let search = state.search.get();
    let mut tabs = AccessNode::new(APP_BAR_TABS, AriaRole::TabList).with_name("View");
    // ★★★★★ R1699 — a nested composite publishes its OWN axis and roster, so a
    // client learns what the inner arrows reach without descending first. The
    // roving is the live one the bar holds, not a fresh copy, or the cursor the
    // wire reports and the cursor the arrows move would be two objects.
    if let Some(inner) = state.inner_cursor_of("shell.appbar", APP_BAR_TABS) {
        tabs = tabs.with_navigation(&inner);
    }
    let mut nodes = Vec::new();
    for (n, name) in TABS.iter().enumerate() {
        let tag = if n == 0 {
            BarChip::Tab0.tag()
        } else {
            BarChip::Tab1.tag()
        };
        tabs = tabs.with_child(tag);
        nodes.push(
            AccessNode::new(tag, AriaRole::Tab)
                .with_name(*name)
                .with_selected(current == *name)
                .with_set_position(n, TABS.len()),
        );
    }
    nodes.insert(0, tabs);
    nodes.insert(
        0,
        with_cursor_declared(
            AccessNode::new("shell.appbar", AriaRole::Toolbar)
                .with_name("Application bar")
                .with_child(APP_BAR_TABS)
                .with_child(BarChip::Source.tag())
                .with_child(BarChip::Capture.tag())
                .with_child(BarChip::Search.tag()),
            state,
        ),
    );
    nodes.push(
        AccessNode::new(BarChip::Source.tag(), AriaRole::Button)
            .with_name("Capture source")
            .with_value(AccessValue::Text(state.source.get()))
            .with_has_popup(HasPopup::Menu),
    );
    // The rate readout moves while nobody touches it, which is what a live
    // region is for — and the only region on this bar that is one.
    nodes.push(
        AccessNode::new(BarChip::Capture.tag(), AriaRole::Status)
            .with_name("Capture")
            .with_value(AccessValue::Text(format!(
                "{}, {}",
                if state.capturing.get() {
                    spec::TRANSPORT
                } else {
                    "Paused"
                },
                spec::RATE,
            )))
            .with_live(AccessLive::Polite),
    );
    nodes.push(
        AccessNode::new(BarChip::Search.tag(), AriaRole::TextInput)
            .with_name("Search")
            // An empty field announces what it is FOR rather than the hint text
            // painted in it: the hint is a placeholder, and reading it as the
            // value would tell a reader the field already holds those words.
            .with_value(AccessValue::Text(search))
            .with_focused(searching),
    );
    nodes
}

/// ★★★★★ R1698 — publish the cursor this composite owns, if the specification
/// gave it one.
///
/// One place, read from the state's own seated `Roving`, so what the wire says
/// the arrows reach and what the arrows actually reach are the same object
/// rather than two lists that agree today. A stop the ring declares with no
/// cursor (the board, whose cursor is spatial) passes through unchanged.
fn with_cursor_declared(node: AccessNode, state: &Rc<ShellState>) -> AccessNode {
    match state.cursor_of(&node.tag) {
        Some(roving) => node.with_navigation(&roving),
        None => node,
    }
}

/// R1699 — the initials the account chip shows, painted and announced from one
/// place so a reader hears what is drawn.
const ACCOUNT_INITIALS: &str = "NE";

/// ★★★★★ R1699 — what a person reads when a verb refuses: **the producer's own
/// sentence**, not the `Debug` spelling of the error that carries it.
///
/// Four call sites here wrote `format!("refused: {why:?}")`, which puts
/// `Rejected(RefusalReason("\"topology\" is reserved for requirement 12 …"))`
/// on the screen — Rust syntax, escaped quotes and all, in front of somebody
/// who asked to place a widget. Found by LOOKING at the round's own demo
/// output, and it is this round's to fix because this round is what made those
/// paths reachable from a keyboard for the first time.
///
/// The rendering itself is `InvokeError`'s `Display`, lifted at the eighth
/// identical site (four here, four in the node lab) per the R727 / R732
/// self-grep mandate — a screen that has to remember not to use `Debug` is a
/// screen that will use `Debug`.
fn refusal_sentence(why: &InvokeError) -> String {
    format!("refused: {why}")
}

/// The tag the two view tabs are announced under. Nothing paints it — the tabs
/// are painted individually and the list is what a reader descends through — so
/// it is anchored by the members it composes.
const APP_BAR_TABS: &str = "shell.appbar.tabs";

/// The rail: seven destinations and the account seat, two of them **locked**.
///
/// Built through [`navigation_link_nodes`] rather than by hand, and the reason
/// this screen is that builder's forcing consumer: a destination that is not
/// available is exactly what its `unavailable` slot exists for. The reason
/// itself is NOT restated here — the seat declares it once on its layout style
/// and the accessibility assembler relays what the disabled cascade resolved, so
/// there is one declaration and no second spelling to drift from it.
fn rail_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let here = state.at();
    let tags: Vec<String> = spec::RAIL
        .iter()
        .map(|seat| format!("shell.rail.{}", seat.key))
        .collect();
    let links: Vec<NavLink<'_>> = spec::RAIL
        .iter()
        .zip(&tags)
        .map(|(seat, tag)| NavLink {
            tag: tag.as_str(),
            label: seat.title,
            state: RadioState::Idle,
            current: seat.key == here,
            focused: false,
            unavailable: None,
        })
        .collect();
    let mut nodes = navigation_link_nodes("shell.rail", "Destinations", &links);
    if let Some(rail) = nodes.first_mut() {
        rail.children.push("shell.rail.account".to_owned());
        *rail = with_cursor_declared(rail.clone(), state);
    }
    // ★★★★★ R1699 — a `group`, not a `button`, and NOT a member of the rail's
    // cursor.
    //
    // Both of those were false claims and the round's own gate is what asked.
    // Nothing presses this seat: no arm of `Hit::at` reaches its rectangle and
    // none of `Hit::of_tag` names it, so it was a control by announcement only —
    // the shape R1694 kept finding from the other side. And the canon settles
    // which direction to repair it in: read out of the reference mockup's own
    // source, the avatar is a plain styled element with no handler, no link and
    // no menu. It says whose session this is. Making it press something would
    // have been inventing a product decision; making it honest costs nothing
    // and removes two lies.
    nodes.push(
        AccessNode::new("shell.rail.account", AriaRole::Group)
            .with_name("Account")
            .with_value(AccessValue::Text(ACCOUNT_INITIALS.to_owned())),
    );
    nodes
}

/// The layout bar: which layout is open, and the two verbs that change the
/// board.
fn sub_bar_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    vec![
        with_cursor_declared(
            AccessNode::new("shell.subbar", AriaRole::Toolbar)
                .with_name("Layout bar")
                .with_child(SubChip::Preset.tag())
                .with_child(SubChip::EditLayout.tag())
                .with_child(SubChip::AddWidget.tag()),
            state,
        ),
        AccessNode::new(SubChip::Preset.tag(), AriaRole::Button)
            .with_name("Layout preset")
            .with_value(AccessValue::Text(state.preset.get()))
            .with_has_popup(HasPopup::Menu)
            .with_expanded(state.preset_open.get()),
        AccessNode::new(SubChip::EditLayout.tag(), AriaRole::Button)
            .with_name(if state.editing.get() {
                "Done"
            } else {
                spec::BOARD_VERBS[0]
            })
            .with_state(AccessState {
                checked: Some(state.editing.get()),
                ..AccessState::default()
            }),
        AccessNode::new(SubChip::AddWidget.tag(), AriaRole::Button).with_name(spec::BOARD_VERBS[1]),
    ]
}

/// Every card placed on the board, and the tags the page region owns them by.
///
/// ★ R1695 — the board's own node is gone from here: the rectangle it described
/// is the **page region** now, and a region that is also a board would be two
/// nodes at one tag. What the board contributes is what it is — a value saying
/// how full it is, and the children.
fn board_nodes(state: &Rc<ShellState>) -> (String, Vec<String>, Vec<AccessNode>) {
    let cards = state.cards.get();
    let value = format!(
        "{} widget(s) placed on a {}-column grid",
        state.placed().len(),
        spec::GRID_COLS,
    );
    let mut children = Vec::new();
    let mut nodes = Vec::new();
    for card in &cards {
        children.push(format!("card.{}", card.id().as_str()));
        nodes.extend(card_nodes(state, card));
    }
    (value, children, nodes)
}

/// ★★ R1695 — the Settings destination, as a reader walks it: four groups, each
/// owning its rows.
///
/// Returns the region's children and the nodes, the same split the board makes,
/// so the page region owns whichever destination is showing.
///
/// The two key rows carry **no reason of their own here**: they declare it once
/// on their layout style and the assembler relays what the disabled cascade
/// resolved. A second spelling is a second thing to drift.
fn settings_nodes(state: &Rc<ShellState>) -> (Vec<String>, Vec<AccessNode>) {
    let mut children = Vec::new();
    let mut nodes = Vec::new();
    for (key, heading) in spec::OPTION_GROUPS {
        let tag = format!("shell.settings.group.{key}");
        children.push(tag.clone());
        let rows = match key {
            "keys" => settings_key_nodes(),
            "appearance" => settings_theme_nodes(state),
            group_key => settings_option_nodes(state, group_key),
        };
        let mut group = AccessNode::new(&tag, AriaRole::Group).with_name(heading);
        for row in &rows {
            group = group.with_child(row.tag.clone());
        }
        nodes.push(group);
        nodes.extend(rows);
    }
    (children, nodes)
}

/// The switch rows of one Settings group.
fn settings_option_nodes(state: &Rc<ShellState>, group: &str) -> Vec<AccessNode> {
    let on = state.options.get();
    spec::OPTIONS
        .iter()
        .enumerate()
        .filter(|(_, option)| option.group == group)
        .map(|(index, option)| {
            AccessNode::new(
                format!("shell.settings.option.{}", option.key),
                AriaRole::Switch,
            )
            .with_name(option.title)
            .with_value(AccessValue::Text(option.gist.to_owned()))
            .with_state(AccessState {
                checked: Some(on[index]),
                ..AccessState::default()
            })
        })
        .collect()
}

/// The two rows whose affordance is booked for a later release.
fn settings_key_nodes() -> Vec<AccessNode> {
    spec::KEY_ROWS
        .iter()
        .map(|row| {
            AccessNode::new(format!("shell.settings.key.{}", row.key), AriaRole::Button)
                .with_name(format!("{} \u{2014} {}", row.title, row.verb))
        })
        .collect()
}

/// The appearance segment: an exclusive choice, so a `radiogroup` of `radio`s.
///
/// ★ R1695 — not two buttons carrying a checked flag, which is what the first
/// draft emitted. A reader told "Dark, button, checked" learns that something is
/// on; told "Dark, radio button, selected, 1 of 2" they learn that choosing the
/// other one turns this off. WAI-ARIA binds `radio` to an owning `radiogroup`
/// and `pinion_a11y::structure` enforces exactly that, so the group is a node
/// rather than a convention.
fn settings_theme_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let dark = theme_word(&state.theme) == "dark";
    let mut group =
        AccessNode::new("shell.settings.theme", AriaRole::RadioGroup).with_name(spec::THEME_ROW.0);
    let mut nodes = Vec::new();
    for (n, name) in spec::THEMES.iter().enumerate() {
        let tag = format!("shell.settings.theme.{n}");
        group = group.with_child(&tag);
        nodes.push(
            AccessNode::new(&tag, AriaRole::RadioButton)
                .with_name(*name)
                .with_state(AccessState {
                    checked: Some((n == 0) == dark),
                    ..AccessState::default()
                }),
        );
    }
    nodes.insert(0, group);
    nodes
}

/// One placed card: the region, its header controls, and its body.
fn card_nodes(state: &Rc<ShellState>, card: &Card) -> Vec<AccessNode> {
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
    let mut region = AccessNode::new(format!("card.{id}"), AriaRole::Group)
        .with_name(card.title())
        .with_value(AccessValue::Text(announce))
        .with_state(AccessState::default())
        .with_child(format!("card.{id}.grip"));
    let mut nodes = vec![
        AccessNode::new(format!("card.{id}.grip"), AriaRole::Button)
            .with_name(format!("Move {}", card.title())),
    ];
    for control in spec::CARD_CHROME {
        let tag = format!("card.{id}.{control}");
        region = region.with_child(tag.clone());
        nodes.push(
            AccessNode::new(tag, AriaRole::Button).with_name(match *control {
                "settings" => "Configure".to_owned(),
                "tear_off" => {
                    if state.is_floating(id) {
                        "Redock".to_owned()
                    } else {
                        "Detach".to_owned()
                    }
                }
                "maximize" => "Maximize".to_owned(),
                _ => format!("Remove {}", card.title()),
            }),
        );
    }
    let body = match def_for_card(id).map(|def| def.kind) {
        Some("packet") => stream_nodes(id),
        Some("decode") => decode_nodes(id),
        Some("keymap") => map_nodes(id),
        Some("filter") => filter_nodes(id),
        _ => Vec::new(),
    };
    for node in &body {
        // The body's own containers are the card's children; everything under
        // them is reached through them.
        if BODY_ROOTS
            .iter()
            .any(|suffix| node.tag == format!("card.{id}.{suffix}"))
        {
            region = region.with_child(node.tag.clone());
        }
    }
    nodes.extend(body);
    nodes.insert(0, region);
    nodes
}

/// The tag suffixes a card body's own top-level containers use — the nodes that
/// become the card region's children.
const BODY_ROOTS: &[&str] = &[
    "grid",
    "tree",
    "bytegrid",
    "query",
    "chips",
    "counts",
    "sparkline",
];

/// The message stream, as a **grid**: a header row of column headers, then one
/// row per message holding one cell per column.
///
/// This is the shape a model-driven item view builds for itself at the floor —
/// measured at 6.11.1, its cell query answers the cell's name, its row, its
/// column and its column header — and the shape a hand-painted table has to
/// build or it has none at all.
fn stream_nodes(id: &str) -> Vec<AccessNode> {
    let rows: Vec<Vec<String>> = spec::STREAM_ROWS
        .iter()
        .map(|(time, kind, name, len)| {
            vec![
                (*time).to_owned(),
                (*kind).to_owned(),
                (*name).to_owned(),
                (*len).to_owned(),
            ]
        })
        .collect();
    table_nodes(id, "Message stream", spec::STREAM_COLUMNS.len(), &rows)
}

/// The identifier map, as a grid on the same shape as the stream.
///
/// ★ The unresolved row's timestamp is painted as an em dash, which is the
/// typographic stand-in for a value that is not knowable — and to somebody
/// reading rather than looking it is a punctuation mark. The cell announces the
/// meaning instead, which the voice census is what asked for: a name with no
/// word in it is a hole.
fn map_nodes(id: &str) -> Vec<AccessNode> {
    let rows: Vec<Vec<String>> = spec::MAP_ROWS
        .iter()
        .map(|(key, path, seen)| {
            let when = if seen.chars().all(|c| !c.is_alphanumeric()) {
                "not known".to_owned()
            } else {
                (*seen).to_owned()
            };
            vec![(*key).to_owned(), (*path).to_owned(), when]
        })
        .collect();
    table_nodes(id, "Identifier map", spec::MAP_COLUMNS.len(), &rows)
}

/// A card body that is a table.
///
/// ★★★★★ Built by [`grid_table_nodes`] rather than by hand. The first draft of
/// this screen hand-rolled the shape — as the sibling capture screen already
/// did — and the two disagreed about where the header row sits: WAI-ARIA counts
/// it in `aria-rowcount`, so it has to be counted in `aria-rowindex` too, and a
/// tree that counts it in one and not the other leaves its header unplaced and
/// its last row unreachable. The rule now lives once, in the builder, where a
/// third table cannot re-derive it differently.
///
/// The column headers are deliberately left unnamed here: they are painted with
/// their own tags, so the name comes from the paint and the two cannot drift.
fn table_nodes(id: &str, name: &str, columns: usize, rows: &[Vec<String>]) -> Vec<AccessNode> {
    let grid_columns: Vec<GridColumn> = (0..columns)
        .map(|c| GridColumn {
            tag: head_cell_tag(id, c),
            sort: None,
        })
        .collect();
    let grid_rows: Vec<GridRow> = rows
        .iter()
        .enumerate()
        .map(|(r, values)| GridRow {
            tag: format!("card.{id}.{}.{r}", row_suffix(id)),
            selected: false,
            state: RadioState::Idle,
            cells: values
                .iter()
                .enumerate()
                .map(|(c, value)| GridCell {
                    tag: cell_tag(id, r, c),
                    name: value.clone(),
                    focused: false,
                    selected: None,
                })
                .collect(),
        })
        .collect();
    grid_table_nodes(
        &format!("card.{id}.grid"),
        name,
        false,
        &format!("card.{id}.head"),
        &grid_columns,
        &grid_rows,
    )
}

/// The tag segment a table card's data rows are painted under.
fn row_suffix(id: &str) -> &'static str {
    if def_for_card(id).map(|def| def.kind) == Some("keymap") {
        "map"
    } else {
        "row"
    }
}

/// The decode inspector: the layer tree, and the bytes it was decoded from.
///
/// ★ The tree is where this beats the floor rather than matching it. Built and
/// run at 6.11.1, a two-column tree announces a row as **two sibling items** —
/// the field and its value are peers, the value reports that it can expand, and
/// the hierarchy is gone: every item is a direct child whatever its depth. Here
/// a field is one item, its value is its value, and the level carries the depth
/// the paint indents by.
fn decode_nodes(id: &str) -> Vec<AccessNode> {
    let mut tree = AccessNode::new(format!("card.{id}.tree"), AriaRole::Tree)
        .with_name("Decoded layers")
        .with_size_of_set(u32::try_from(spec::DECODE_ROWS.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (n, (depth, key, value)) in spec::DECODE_ROWS.iter().enumerate() {
        let tag = format!("card.{id}.tree.{n}");
        tree = tree.with_child(tag.clone());
        let (place, siblings) = sibling_place(n);
        let mut item = AccessNode::new(tag, AriaRole::TreeItem)
            .with_name(*key)
            .with_level(*depth + 1)
            .with_set_position(place, siblings)
            .with_selected(n == spec::DECODE_SELECTED);
        if !value.is_empty() {
            item = item.with_value(AccessValue::Text((*value).to_owned()));
        }
        // A layer heading is what folds; a field under it does not.
        if *depth == 0 {
            item = item.with_expanded(true);
        }
        nodes.push(item);
    }
    nodes.insert(0, tree);
    nodes.extend(byte_nodes(id));
    nodes
}

/// Where one decode row sits **among its own siblings** — the pair a flat index
/// cannot give, and the one a reader is told.
fn sibling_place(n: usize) -> (usize, usize) {
    let depth = spec::DECODE_ROWS[n].0;
    let siblings: Vec<usize> = spec::DECODE_ROWS
        .iter()
        .enumerate()
        .filter(|(m, row)| row.0 == depth && same_parent(*m, n))
        .map(|(m, _)| m)
        .collect();
    let place = siblings.iter().position(|m| *m == n).unwrap_or(0);
    (place, siblings.len())
}

/// Whether two decode rows of the same depth hang under the same heading.
fn same_parent(a: usize, b: usize) -> bool {
    let parent = |n: usize| {
        let depth = spec::DECODE_ROWS[n].0;
        (0..n).rev().find(|m| spec::DECODE_ROWS[*m].0 < depth)
    };
    parent(a) == parent(b)
}

/// The captured frame as a grid: one row per painted line, one cell per byte,
/// and the bytes the selected field was read from announced as selected.
fn byte_nodes(id: &str) -> Vec<AccessNode> {
    let per_line = 4;
    let lines = spec::DECODE_BYTES.len();
    let mut grid = AccessNode::new(format!("card.{id}.bytegrid"), AriaRole::Grid)
        .with_name("Captured bytes")
        .with_row_count(u32::try_from(lines).unwrap_or(u32::MAX))
        .with_column_count(u32::try_from(per_line).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    let (from, to) = spec::DECODE_SELECTED_SPAN;
    for (line, bytes) in spec::DECODE_BYTES.iter().enumerate() {
        let row_tag = format!("card.{id}.bytes.{line}");
        grid = grid.with_child(row_tag.clone());
        // The row is named by the offset it starts at, which is what the strip
        // paints in its left column and what a reader counts from.
        let mut row = AccessNode::new(row_tag, AriaRole::Row)
            .with_name(format!("{:04x}", line * per_line))
            .with_row(line);
        for (column, byte) in bytes.iter().enumerate() {
            let index = line * per_line + column;
            let tag = format!("card.{id}.byte.{index}");
            row = row.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::GridCell)
                    .with_name(format!("{byte:02x}"))
                    .with_row(line)
                    .with_column(column)
                    .with_selected(index >= from && index < to),
            );
        }
        nodes.push(row);
    }
    nodes.insert(0, grid);
    nodes
}

/// The search and filter card: the query, the saved chips, and the three counts
/// whose **relation** is the point of the card.
fn filter_nodes(id: &str) -> Vec<AccessNode> {
    let mut nodes = vec![
        AccessNode::new(format!("card.{id}.query"), AriaRole::TextInput)
            .with_name("Query")
            .with_value(AccessValue::Text(spec::FILTER_QUERY.to_owned())),
    ];
    let mut chips =
        AccessNode::new(format!("card.{id}.chips"), AriaRole::Group).with_name("Saved filters");
    for (n, (name, on)) in spec::FILTER_CHIPS.iter().enumerate() {
        let tag = format!("card.{id}.chip.{n}");
        chips = chips.with_child(tag.clone());
        // A saved filter is on or off, which WAI-ARIA reflects as a toggle
        // button's pressed state rather than as a separate control kind.
        nodes.push(
            AccessNode::new(tag, AriaRole::Button)
                .with_name(*name)
                .with_state(AccessState {
                    checked: Some(*on),
                    ..AccessState::default()
                })
                .with_set_position(n, spec::FILTER_CHIPS.len()),
        );
    }
    let mut counts =
        AccessNode::new(format!("card.{id}.counts"), AriaRole::Group).with_name("Match counts");
    for (n, (value, what)) in spec::FILTER_STATS.iter().enumerate() {
        let tag = format!("card.{id}.stat.{n}");
        counts = counts.with_child(tag.clone());
        // The word is the name and the number is the value: a reader told only
        // "12,418" has been told which of three numbers it is by position, and
        // position is exactly what somebody not looking at the card does not
        // have.
        nodes.push(
            AccessNode::new(tag, AriaRole::Status)
                .with_name(*what)
                .with_value(AccessValue::Text((*value).to_owned())),
        );
    }
    nodes.push(
        AccessNode::new(format!("card.{id}.sparkline"), AriaRole::Group)
            .with_name("Matched over time")
            .with_value(AccessValue::Text(series_reading(&MATCH_SERIES))),
    );
    nodes.push(chips);
    nodes.push(counts);
    nodes
}

/// What a sparkline says to somebody who cannot see it: how many samples, the
/// range they cover, and where it ended.
fn series_reading(series: &[f64]) -> String {
    let low = series.iter().copied().fold(f64::INFINITY, f64::min);
    let high = series.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let last = series.last().copied().unwrap_or(0.0);
    format!(
        "{} samples, {low:.0} to {high:.0}, latest {last:.0}",
        series.len()
    )
}

/// The palette: three sections of catalogue entries, **nine of them locked**.
///
/// Every entry is a `listitem` whatever its tier, and its place in the set is
/// counted over the whole catalogue — because a locked seat is a seat. Dropping
/// the locked ones would make the palette announce four entries while the panel
/// shows thirteen and its own footer says nine are reserved.
///
/// The reason a locked entry is locked is **not restated here**: the row
/// declares it once on its layout style, and the accessibility assembler relays
/// what the disabled cascade resolved. One declaration, and the wire, the
/// accessibility tree and the ink cannot disagree.
fn palette_nodes(state: &Rc<ShellState>) -> Vec<AccessNode> {
    let mut list = AccessNode::new("shell.palette", AriaRole::List)
        .with_name(spec::PALETTE_TITLE)
        .with_size_of_set(u32::try_from(spec::CATALOGUE.len()).unwrap_or(u32::MAX));
    let mut nodes = Vec::new();
    for (key, title, _tier) in spec::SECTIONS {
        let section_tag = format!("shell.palette.section.{key}");
        list = list.with_child(section_tag.clone());
        let mut section = AccessNode::new(section_tag, AriaRole::Group).with_name(*title);
        for (n, def) in spec::CATALOGUE.iter().enumerate() {
            if def.section != *key {
                continue;
            }
            let tag = format!("shell.palette.{}", def.kind);
            section = section.with_child(tag.clone());
            nodes.push(
                AccessNode::new(tag, AriaRole::ListItem)
                    .with_name(def.label)
                    .with_value(AccessValue::Text(def.gist.to_owned()))
                    .with_set_position(n, spec::CATALOGUE.len()),
            );
        }
        nodes.push(section);
    }
    list = with_cursor_declared(
        list.with_child("shell.palette.placed")
            .with_child("shell.palette.reserved"),
        state,
    );
    nodes.push(
        AccessNode::new("shell.palette.placed", AriaRole::Status)
            .with_name("Placed")
            .with_value(AccessValue::Text(format!(
                "{} of {}",
                state.placed().len(),
                spec::placeable_count()
            ))),
    );
    nodes.push(
        AccessNode::new("shell.palette.reserved", AriaRole::Status)
            .with_name("Reserved")
            .with_value(AccessValue::Text(spec::reserved_count().to_string())),
    );
    nodes.insert(0, list);
    nodes
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
